TermKey for macOS
================

This disk image contains Install TermKey.pkg, the signed and notarized installer for
TermKey, its command-line tool, and its uninstaller.

Requirements
------------

TermKey supports Apple Silicon Macs running macOS 11 or newer.

Install
-------

1. Open Install TermKey.pkg.
2. Follow the macOS installer steps.
3. The installer places TermKey.app and Uninstall TermKey.app in /Applications.
4. It also places termkey at /usr/local/bin/termkey.
5. Open a new Terminal window and run termkey normally.

Uninstall
---------

To remove TermKey later, open Uninstall TermKey.app from /Applications. It removes
the app bundle, CLI binaries, Chrome integration files installed by TermKey, and the
installer receipt. Your ~/.termkey vault data is left untouched.
