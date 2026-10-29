# Settings interface

Settings uses the existing browser sidebar for navigation and the main area
for its content. It opens on General in a fresh session and retains the last
section during that session. Search indexes individual preferences and the
reserved section destinations; selecting a preference reveals and highlights
its row. There is no separate gallery or Studio application.

## Structure

`frame/src/features/settings` owns explicit page compositions and reusable
form/editor patterns. The catalog supplies labels, descriptions, defaults,
options and search metadata; it does not generate complete pages.

Browser pages include General, Appearance, New Tab, Tabs & spaces, Profiles,
Search, Privacy & security, Passwords & autofill, Downloads, Keyboard &
accessibility, Languages, Time & focus, Performance, Account, Developers,
Documentation, and About & advanced. Extensions remains in the browser's
unified menu instead of duplicating management inside Settings.

Agents, Models, Plugins, MCP servers, Skills, and Memory are navigation
reservations. They intentionally contain only their headings. They mount no
configuration forms, models, services, or execution systems.

## Interaction and authority

Already-connected appearance preferences continue through their existing
Rust commands. Other browser settings use the profile-scoped, in-memory
preview store. They survive page navigation in the current session, but do
not change system settings, download paths, browser permissions, search
providers, imports, updates, authentication, or external tool access.

Dialogs distinguish preview changes from real effects. Cancelling restores
the captured draft for the same profile; applying retains only preview state.
No import, export, cleanup, update, default-browser, or folder-selection
service is called by these controls. Profile photographs remain explicit,
bounded user-selected previews under the existing image restrictions.

Language options include ISO language codes and common regional/script
variants, with English display names. Preferred website languages can be
searched, added, reordered, and removed, with one required selection and a
maximum of twenty. Interface-language selection updates a preview value only:
it never changes the Paraglide locale or translates the interface.

Density, text size, and contrast previews update the rendered interface for
the current session. Actual backend preference support can be added later
without treating preview values as persisted state.

## Profile shortcuts

Keyboard & accessibility includes a Profiles section for rebinding Next profile
and Previous profile. The defaults are Ctrl+Cmd+P and Ctrl+Cmd+Shift+P on macOS,
and F9 and Shift+F9 on Windows. Both commands cycle through the persistent
profile roster and wrap around; a single profile leaves the selection unchanged.
From a private window, cycling starts from its regular profile and leaves private
browsing when switching to another profile. Repeated steps while Settings returns
to Browse accumulate from the queued destination rather than losing a keypress.

![Profile shortcuts in Keyboard & accessibility](../../.github/assets/profile-shortcuts.png)

Keyboard page rendered in a WebKit component test with mocked native bindings.

## Verification

The frame check covers types, lint, formatting, and tests. Focused tests cover
language ordering, validation, bounded selection, reserved-section search,
preview rollback, profile isolation, and selective preview reset. Production
build compilation remains a separate check. Native visual and interactive
review is required before claiming final visual acceptance.
