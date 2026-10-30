//! Permissions of the APIs the browser implements itself. WebKit drops
//! permissions it doesn't know from the manifest and never grants them, so
//! these grants live here.

use std::collections::BTreeSet;
use std::rc::Rc;

use objc2_web_kit::{WKWebExtension, WKWebExtensionContext};
use serde_json::{json, Value};

use crate::runtime::{Grants, Shared};
use crate::{events, json};

/// Kept in sync with `BROWSER_PERMISSIONS` in the compatibility layer.
pub const BROWSER_PERMISSIONS: &[&str] = &["idle", "notifications", "privacy"];

pub(crate) fn is_browser_permission(name: &str) -> bool {
    BROWSER_PERMISSIONS.contains(&name)
}

/// What the manifest asks for among the browser's permissions, required and
/// optional.
pub(crate) fn declared(extension: &WKWebExtension) -> (Vec<String>, Vec<String>) {
    let manifest = unsafe { extension.manifest() };
    let manifest = json::from_object(Some(&manifest));
    let list = |key: &str| -> Vec<String> {
        manifest
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|name| is_browser_permission(name))
            .map(str::to_owned)
            .collect()
    };
    (list("permissions"), list("optional_permissions"))
}

pub(crate) fn initial(extension: &WKWebExtension, grants: &Grants) -> BTreeSet<String> {
    match grants {
        Grants::Requested => declared(extension).0.into_iter().collect(),
        Grants::Explicit { permissions, .. } => permissions
            .iter()
            .filter(|name| is_browser_permission(name))
            .cloned()
            .collect(),
    }
}

pub(crate) fn granted(shared: &Shared, extension: &str, permission: &str) -> bool {
    shared
        .loaded
        .borrow()
        .get(extension)
        .is_some_and(|loaded| loaded.browser_permissions.borrow().contains(permission))
}

pub(crate) fn extension_id(context: &WKWebExtensionContext) -> String {
    unsafe { context.uniqueIdentifier() }.to_string()
}

fn requested(message: &Value) -> Vec<String> {
    message
        .get("permissions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|name| is_browser_permission(name))
        .take(BROWSER_PERMISSIONS.len())
        .map(str::to_owned)
        .collect()
}

/// `{ granted, optional }` for the compatibility layer's permission calls.
pub(crate) fn state(shared: &Shared, context: &WKWebExtensionContext) -> Value {
    let id = extension_id(context);
    let granted: Vec<String> = shared
        .loaded
        .borrow()
        .get(&id)
        .map(|loaded| {
            loaded
                .browser_permissions
                .borrow()
                .iter()
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let (required, optional) = declared(&*unsafe { context.webExtension() });
    json!({ "granted": granted, "declared": required.into_iter().chain(optional).collect::<Vec<_>>() })
}

/// `permissions.request` for browser permissions: only ones the manifest
/// declares, asked through the same prompt as WebKit's own.
pub(crate) fn request(
    shared: &Rc<Shared>,
    context: &WKWebExtensionContext,
    message: &Value,
    done: Box<dyn FnOnce(Result<Value, String>)>,
) {
    let id = extension_id(context);
    let wanted = requested(message);
    let (required, optional) = declared(&*unsafe { context.webExtension() });
    if let Some(name) = wanted
        .iter()
        .find(|name| !required.contains(name) && !optional.contains(name))
    {
        return done(Err(format!(
            "Only permissions specified in the manifest may be requested. '{name}' was not."
        )));
    }
    let missing: Vec<String> = wanted
        .into_iter()
        .filter(|name| !granted(shared, &id, name))
        .collect();
    if missing.is_empty() {
        return done(Ok(Value::Bool(true)));
    }
    let weak = Rc::downgrade(shared);
    let extension = id.clone();
    crate::access::request(
        shared,
        &id,
        missing.clone(),
        Vec::new(),
        Box::new(move |allowed| {
            if allowed {
                if let Some(shared) = weak.upgrade() {
                    grant(&shared, &extension, &missing);
                }
            }
            done(Ok(Value::Bool(allowed)));
        }),
    );
}

fn grant(shared: &Rc<Shared>, extension: &str, permissions: &[String]) {
    let added: Vec<String> = match shared.loaded.borrow().get(extension) {
        Some(loaded) => {
            let mut set = loaded.browser_permissions.borrow_mut();
            permissions
                .iter()
                .filter(|name| set.insert((*name).clone()))
                .cloned()
                .collect()
        }
        None => return,
    };
    if !added.is_empty() {
        events::emit(
            shared,
            extension,
            "permissions.onAdded",
            json!([{ "permissions": added, "origins": [] }]),
            None,
        );
    }
}

pub(crate) fn remove(
    shared: &Rc<Shared>,
    context: &WKWebExtensionContext,
    message: &Value,
) -> Value {
    let id = extension_id(context);
    let wanted = requested(message);
    let (required, _) = declared(&*unsafe { context.webExtension() });
    if wanted.iter().any(|name| required.contains(name)) {
        return Value::Bool(false);
    }
    let removed: Vec<String> = match shared.loaded.borrow().get(&id) {
        Some(loaded) => {
            let mut set = loaded.browser_permissions.borrow_mut();
            wanted.into_iter().filter(|name| set.remove(name)).collect()
        }
        None => Vec::new(),
    };
    if !removed.is_empty() {
        shared.host().access_removed(&id, &removed, &[]);
        crate::privacy::forget_if_revoked(shared, &id);
        events::emit(
            shared,
            &id,
            "permissions.onRemoved",
            json!([{ "permissions": removed, "origins": [] }]),
            None,
        );
    }
    Value::Bool(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_compatibility_layer_lists_the_same_permissions() {
        let names: Vec<String> = super::BROWSER_PERMISSIONS
            .iter()
            .map(|name| format!("{name:?}"))
            .collect();
        let line = format!(
            "const BROWSER_PERMISSIONS = new Set([{}]);",
            names.join(", ")
        );
        assert!(crate::compat::SCRIPT.contains(&line), "{line}");
    }
}
