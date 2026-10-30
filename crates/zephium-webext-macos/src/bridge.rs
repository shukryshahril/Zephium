//! Native half of the compatibility layer: `runtime.sendNativeMessage` to
//! [`APPLICATION`] with `{ api, ... }` lands here.

use std::rc::Rc;

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{NSError, NSString};
use objc2_web_kit::{
    WKWebExtensionContext, WKWebExtensionPermission, WKWebExtensionPermissionClipboardWrite,
};
use serde_json::Value;

use crate::runtime::Shared;
use crate::{api, error, idle, json, notifications, offscreen, privacy, LogLevel};

pub(crate) const APPLICATION: &str = "app.zephium.webext";

pub(crate) fn handle(
    shared: &Rc<Shared>,
    context: &WKWebExtensionContext,
    message: &AnyObject,
    reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
) {
    let message = json::from_object(Some(message));
    if message.get("api").and_then(Value::as_str) == Some("identity.launch") {
        // A silent refresh must never become an unsolicited visible login
        // tab. Enforce this natively as well as in the compatibility layer.
        if message.get("interactive").and_then(Value::as_bool) != Some(true) {
            let error = error("Non-interactive authentication is not supported.");
            reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
            return;
        }
        let url = message
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let reply = reply.copy();
        let extension = unsafe { context.uniqueIdentifier() }.to_string();
        shared.host().start_auth_flow(
            &extension,
            url,
            Box::new(move |result| match result {
                Ok(url) => {
                    let object = json::to_object(&serde_json::json!({ "url": url }));
                    reply.call((Retained::as_ptr(&object).cast_mut(), std::ptr::null_mut()));
                }
                Err(message) => {
                    let error = error(&message);
                    reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
                }
            }),
        );
        return;
    }
    if message.get("api").and_then(Value::as_str) == Some("offscreen.create") {
        let url = message
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let reply = reply.copy();
        offscreen::create(
            shared,
            context,
            url,
            Box::new(move |result| match result {
                Ok(()) => {
                    let object = json::null();
                    reply.call((Retained::as_ptr(&object).cast_mut(), std::ptr::null_mut()));
                }
                Err(message) => {
                    let error = error(&message);
                    reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
                }
            }),
        );
        return;
    }
    if message.get("api").and_then(Value::as_str) == Some("permissions.request") {
        let reply = reply.copy();
        api::request(
            shared,
            context,
            &message,
            Box::new(move |result| respond(&reply, result)),
        );
        return;
    }
    let id = extension(context);
    let gate = |permission: &str| {
        if api::granted(shared, &id, permission) {
            Ok(())
        } else {
            Err(format!(
                "The extension doesn't have the '{permission}' permission."
            ))
        }
    };
    let interval = message.get("interval").and_then(Value::as_u64);
    let result = match message.get("api").and_then(Value::as_str) {
        Some("permissions.state") => Ok(api::state(shared, context)),
        Some("permissions.remove") => Ok(api::remove(shared, context, &message)),
        Some("idle.query") => gate("idle").map(|()| Value::from(idle::query(interval))),
        Some("idle.watch") => gate("idle").map(|()| {
            idle::watch(shared, &id, interval);
            Value::Null
        }),
        Some("notifications.create") => gate("notifications").and_then(|()| {
            let name = unsafe { context.webExtension().displayName() }
                .map(|name| name.to_string())
                .unwrap_or_default();
            notifications::create(shared, &id, &name, &message)
        }),
        Some("notifications.clear") => gate("notifications").map(|()| {
            let target = message
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Value::Bool(notifications::clear(shared, &id, target))
        }),
        Some("notifications.getAll") => {
            gate("notifications").map(|()| notifications::all(shared, &id))
        }
        Some("notifications.permission") => Ok(Value::from(notifications::permission())),
        Some("privacy.get") => gate("privacy").and_then(|()| privacy::get(shared, &id, &message)),
        Some("privacy.set") => gate("privacy").and_then(|()| privacy::set(shared, &id, &message)),
        Some("privacy.clear") => {
            gate("privacy").and_then(|()| privacy::clear(shared, &id, &message))
        }
        Some("log") => {
            let level = match message.get("level").and_then(Value::as_str) {
                Some("error") => LogLevel::Error,
                Some("warning") => LogLevel::Warning,
                _ => LogLevel::Info,
            };
            let text = message
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let text: String = text.chars().take(2000).collect();
            shared.log(context, level, &text);
            Ok(Value::Null)
        }
        Some("trace") => Ok(Value::Bool(crate::tracing())),
        Some("worker.started") => {
            let id = extension(context);
            if crate::tracing() {
                eprintln!("webext-trace: worker of {id} started");
            }
            crate::worker_gone(shared, &id);
            crate::lifetime::started(shared, &id);
            Ok(Value::Null)
        }
        Some("offscreen.close") => Ok(Value::Bool(offscreen::close(shared, &extension(context)))),
        Some("offscreen.has") => Ok(Value::Bool(offscreen::has(shared, &extension(context)))),
        Some("clipboard.write")
            if permitted(context, unsafe { WKWebExtensionPermissionClipboardWrite }) =>
        {
            let text = message
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            let written = pasteboard
                .setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
            Ok(Value::Bool(written))
        }
        Some("clipboard.read") if declares(context, "clipboardRead") => {
            let text = NSPasteboard::generalPasteboard()
                .stringForType(unsafe { NSPasteboardTypeString })
                .map(|text| text.to_string());
            Ok(text.map_or(Value::Null, Value::String))
        }
        Some(api) => Err(format!("{api} is not supported")),
        None => Err("missing api".to_owned()),
    };
    respond(reply, result);
}

fn respond(reply: &DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>, result: Result<Value, String>) {
    match result {
        Ok(value) => {
            let object = json::to_object(&value);
            reply.call((Retained::as_ptr(&object).cast_mut(), std::ptr::null_mut()));
        }
        Err(message) => {
            let error = error(&message);
            reply.call((std::ptr::null_mut(), Retained::as_ptr(&error).cast_mut()));
        }
    }
}

fn extension(context: &WKWebExtensionContext) -> String {
    unsafe { context.uniqueIdentifier() }.to_string()
}

fn permitted(
    context: &WKWebExtensionContext,
    permission: Option<&WKWebExtensionPermission>,
) -> bool {
    permission.is_some_and(|permission| unsafe { context.hasPermission(permission) })
}

/// Whether the manifest asks for `permission`, for permissions WebKit
/// doesn't know and so never reports as granted.
fn declares(context: &WKWebExtensionContext, permission: &str) -> bool {
    let manifest = unsafe { context.webExtension().manifest() };
    let manifest = json::from_object(Some(&manifest));
    manifest
        .get("permissions")
        .and_then(Value::as_array)
        .is_some_and(|permissions| {
            permissions
                .iter()
                .any(|entry| entry.as_str() == Some(permission))
        })
}
