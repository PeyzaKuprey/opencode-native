TERMUX_PKG_HOMEPAGE=https://github.com/PeyzaKuprey/opencode-native
TERMUX_PKG_DESCRIPTION="Lightweight native OpenCode-style coding agent"
TERMUX_PKG_LICENSE="MIT"
TERMUX_PKG_MAINTAINER="@PeyzaKuprey"
TERMUX_PKG_VERSION=0.1.0
TERMUX_PKG_SRCURL=git+https://github.com/PeyzaKuprey/opencode-native
TERMUX_PKG_GIT_BRANCH=master
TERMUX_PKG_BUILD_IN_SRC=true
TERMUX_PKG_AUTO_UPDATE=true

termux_step_make() {
	termux_setup_rust
	cargo build --jobs $TERMUX_PKG_MAKE_PROCESSES --target $CARGO_TARGET_NAME --release
}

termux_step_make_install() {
	install -Dm755 -t $TERMUX_PREFIX/bin target/${CARGO_TARGET_NAME}/release/opencode-native
	ln -sf opencode-native $TERMUX_PREFIX/bin/ocn
}
