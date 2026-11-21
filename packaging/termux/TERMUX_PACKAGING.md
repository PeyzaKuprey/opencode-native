# Termux packaging notes

This repo includes a local `.deb` builder: `scripts/build-termux-deb.sh`.

## Local install (device)

```sh
./scripts/build-termux-deb.sh aarch64
pkg install ./dist/opencode-native_0.1.0_aarch64.deb
```

## Official Termux repo (`pkg install opencode-native`)

To be installable from Termux repositories, a package recipe must be added and accepted in Termux packaging infrastructure (typically `termux-packages`). This requires:

1. Stable source release/tag and license.
2. Build recipe (`build.sh`) with dependencies, checksum, and install steps.
3. CI/build validation on Termux builders for target arches.
4. Maintainer review/merge.

This environment can prepare the recipe, but publishing to official repos requires upstream submission and approval.
