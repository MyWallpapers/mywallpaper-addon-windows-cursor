# Windows Cursor

Windows Cursor applies a custom normal pointer to the current Windows user
session through a supervised `process-v2` companion. It uses documented Win32
cursor APIs, needs no administrator rights, and restores the user's configured
Windows cursor scheme when disabled or shut down.

The add-on intentionally leaves text-selection, resize, busy, and accessibility
cursors untouched. This preserves Windows semantics while making the pointer
visible above every wallpaper and application.

Native execution requires explicit consent for the exact release version and
digest. The central OIDC workflow rebuilds the Windows companion twice before admission.

## License

MIT. See [LICENSE](LICENSE).

## Publishing

Merge the source and matching manifest/package version into the reviewed default
branch, wait for quality checks, then push a new immutable `v<version>` tag.
Open this add-on's management page in MyWallpaper and select that tag to request
publication with an active lifetime entitlement.

MyWallpaper resolves the exact public repository and commit, dispatches its
pinned central workflow, rebuilds and verifies the artifacts, and publishes the
immutable transport from the platform repository. The add-on repository needs
no publication workflow or MyWallpaper credential. Do not pre-create a GitHub
release: a source tag alone does not publish the add-on to the catalogue.

Each accepted newer release is available for new installations. Existing
wallpapers remain pinned to their exact release until explicitly changed.
