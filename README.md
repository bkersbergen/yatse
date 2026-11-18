# Yet AnoTher Steam Editor (YATSE)

![YATSE app showing the Steam library title editor](assets/yatse_screenshot.png)

A local title editor for Steam games. Full game titles can be edited, but the main purpose is to add release years to titles, making major entries in a game series easier to tell apart.

 __   __   _ _____ ___ ___
 \ \ / /  /_\_   _/ __| __|
  \ V /  / _ \| | \__ \ _|
   |_|  /_/ \_\_| |___/___|

Use at your own risk.

Download the Windows x64 executable or portable ZIP from [GitHub Releases](https://github.com/bkersbergen/yatse/releases). Extract the ZIP into a writable folder and run `yatse.exe`. Settings and backups are stored beside the executable. Close Steam before applying title changes.

To publish a release, set the version in `Cargo.toml`, update `Cargo.lock` with `cargo check`, and commit and push those changes. Then create and push the matching tag:

```sh
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions tests and builds the Windows app, then publishes the executable, portable ZIP, and SHA-256 checksums. Tags must match the Cargo version. Tags such as `v0.2.0-beta.1` create prereleases. Pushes to `main` and manual runs also produce downloadable build artifacts on the [Actions page](https://github.com/bkersbergen/yatse/actions), without publishing a release.
