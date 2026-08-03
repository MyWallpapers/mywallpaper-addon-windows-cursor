# Windows Cursor

Windows Cursor applies a custom normal pointer to the current Windows user
session through a supervised `process-v2` companion. It uses documented Win32
cursor APIs, needs no administrator rights, and restores the user's configured
Windows cursor scheme when disabled or shut down.

The add-on intentionally leaves text-selection, resize, busy, and accessibility
cursors untouched. This preserves Windows semantics while making the pointer
visible above every wallpaper and application.

Native execution requires explicit consent for the exact release version and
digest. The immutable OIDC workflow rebuilds both Windows architectures twice
before admission.

## License

MIT. See [LICENSE](LICENSE).
