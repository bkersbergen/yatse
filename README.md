# Yet AnoTher Steam Editor (YATSE)

![YATSE app showing the Steam library title editor](assets/yatse_screenshot.png)

A local title editor for Steam games. Full game titles can be edited, but the main purpose is to add release years to titles, making major entries in a game series easier to tell apart.

 __   __   _ _____ ___ ___
 \ \ / /  /_\_   _/ __| __|
  \ V /  / _ \| | \__ \ _|
   |_|  /_/ \_\_| |___/___|

Use at your own risk.

Download the Windows x64 executable or portable ZIP from [GitHub Releases](https://github.com/bkersbergen/yatse/releases). Extract the ZIP into a writable folder and run `yatse.exe`. Settings and backups are stored beside the executable. Close Steam before applying title changes.

Every successful build on `main` automatically publishes a Windows release with the executable, portable ZIP, and SHA-256 checksums. Releases use unique tags such as `build-2-1` and point to the exact commit that was built. Manual runs on `main` also publish releases; failed builds do not.

For a named version release, set the version in `Cargo.toml`, update `Cargo.lock` with `cargo check`, commit and push, then push the matching tag:

```sh
git tag v0.1.0
git push origin v0.1.0
```

Version tags must match the Cargo version. Tags such as `v0.2.0-beta.1` create prereleases. Build artifacts and logs are also available on the [Actions page](https://github.com/bkersbergen/yatse/actions).
