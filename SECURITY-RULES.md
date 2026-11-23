# Security rules

Rules every change to Scrawl must keep. The security gate's review blocks a change that breaks
one, and the tests named under each rule check it on every build (`cargo test`). A rule changes
only with the owner's OK.

## 1. Only this computer can reach Scrawl's servers

Why: a capture holds your screen and your voice. Nothing else on the network may upload to,
read from or steer the ingest server, the `/sdk.js` routes or the autocrop playground.
Enforced by: `src/phone.rs`, "every_listener_binds_loopback_only"

## 2. Uploads and private routes need the ingest token

Why: any program or web page on this machine can reach a loopback port; without the token it
must not upload a capture or list which pages and sessions are registered.
Enforced by: `src/phone.rs`, "ingest_rejects_missing_or_wrong_tokens_with_401_and_accepts_valid_token";
`src/web.rs`, "http_routes_serve_the_script_and_check_registrations"

## 3. Websites can't register pages or steer captures

Why: a page you open in the browser (or one that rebinds its domain to 127.0.0.1) must not
decide which agent session your captures go to. Only pages served from this machine register
without the token.
Enforced by: `src/web.rs`, "loopback_origins_register_without_the_token_and_others_need_it" and
"unauthenticated_pages_register_only_their_own_origin"

## 4. The ingest token is readable by you alone

Why: it is the key to rule 2. `config.json` is written whole and is `0600` from the moment it
exists.
Enforced by: `src/handoff.rs`, "token_generation_persists_with_0600_and_preserves_other_keys" and
"auto_send_defaults_on_and_the_toggle_keeps_other_keys"

## 5. A capture only ever reads files inside its own folder

Why: a `.scrawl` bundle can come from someone else. The files its manifest names are sent to
agents, pasted and copied, so none may lead outside the bundle (absolute paths, `..`, symlinks).
Enforced by: `src/handoff.rs`, "manifests_cannot_name_files_outside_their_bundle"

## 6. Cleanup moves captures to the Trash, and only captures

Why: retention runs by itself every hour. It never deletes media outright, never acts on a
config it can't read, and never follows a link out of the captures folder.
Enforced by: `src/retention.rs`, "a_trash_that_refuses_the_media_leaves_them_in_the_bundle",
"media_a_crash_left_staged_go_back_into_their_bundle",
"a_symlink_named_like_a_bundle_is_never_cleaned_up" and "cleanup_refuses_a_config_it_cannot_read"

## 7. Release builds run only pinned, checked tools

Why: what packs the AppImage ends up on users' machines. Every tool the packaging downloads is
pinned to a release or commit and checked against its SHA-256 before it runs.
Enforced by: review only (`scripts/linux/appimage.sh` stops on a mismatch)
