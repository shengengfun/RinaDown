#!/bin/sh
# 手工构建 OpenWrt ipk（不依赖 OpenWrt SDK）。
# ipk = tar.gz(debian-binary + control.tar.gz + data.tar.gz)，OpenWrt opkg 标准外层格式。
#
# 用法：
#   build_ipk.sh server <version> <binary> <arch,arch,...> <out_dir>
#   build_ipk.sh luci   <version> <out_dir>
#
# server 子命令对同一份载荷按 arch 列表出多个 ipk（aarch64 子架构 opkg 严格校验，
# 静态 musl 二进制本身通用，只是 control 里的 Architecture 标签不同）。
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

# tar 归一化参数：root 属主、无扩展头，确保 opkg 可解
TAR="tar --format=gnu --owner=0 --group=0 --numeric-owner"

make_ipk() {
	# $1=staging 目录（含 control/ 与 data/ 两个子目录） $2=输出 ipk 路径
	staging=$1
	out=$2
	echo "2.0" > "$staging/debian-binary"
	$TAR -C "$staging/control" -czf "$staging/control.tar.gz" .
	$TAR -C "$staging/data" -czf "$staging/data.tar.gz" .
	$TAR -C "$staging" -czf "$out" ./debian-binary ./control.tar.gz ./data.tar.gz
	echo "built: $out"
}

build_server() {
	# 二进制自带 Web UI（编译期内嵌），载荷里没有 webroot 目录。
	VERSION=$1 BIN=$2 ARCHES=$3 OUTDIR=$4
	[ -f "$BIN" ] || { echo "binary not found: $BIN" >&2; exit 1; }
	mkdir -p "$OUTDIR"

	work=$(mktemp -d)
	trap 'rm -rf "$work"' EXIT

	# ── data 载荷（各 arch 共用） ──
	data="$work/data"
	mkdir -p "$data/usr/bin" "$data/etc/init.d" "$data/etc/config"
	cp "$BIN" "$data/usr/bin/rinadown-server"
	chmod 755 "$data/usr/bin/rinadown-server"
	cp "$SCRIPT_DIR/files/rinadown.init" "$data/etc/init.d/rinadown"
	chmod 755 "$data/etc/init.d/rinadown"
	cp "$SCRIPT_DIR/files/rinadown.config" "$data/etc/config/rinadown"

	size=$(du -sk "$data" | cut -f1)

	for arch in $(echo "$ARCHES" | tr ',' ' '); do
		staging="$work/pkg-$arch"
		mkdir -p "$staging/control"
		ln -s "$data" "$staging/data" 2>/dev/null || cp -r "$data" "$staging/data"

		cat > "$staging/control/control" <<-EOF
		Package: rinadown-server
		Version: $VERSION
		Depends: libc
		Section: net
		Architecture: $arch
		Installed-Size: ${size}k
		Maintainer: RinaDown <https://rinadown.zerx.dev>
		License: AGPL-3.0
		Description: Blazing fast multi-protocol download manager (headless server).
		 HTTP/HTTPS/FTP/BitTorrent/HLS downloads with intelligent segmentation,
		 served with a full web UI on port 17800. Statically linked (musl).
		EOF

		cat > "$staging/control/conffiles" <<-EOF
		/etc/config/rinadown
		EOF

		cat > "$staging/control/postinst" <<-'EOF'
		#!/bin/sh
		[ -n "$IPKG_INSTROOT" ] || {
			/etc/init.d/rinadown enable
			/etc/init.d/rinadown start
		}
		exit 0
		EOF

		cat > "$staging/control/prerm" <<-'EOF'
		#!/bin/sh
		[ -n "$IPKG_INSTROOT" ] || {
			/etc/init.d/rinadown stop
			/etc/init.d/rinadown disable
		}
		exit 0
		EOF
		chmod 755 "$staging/control/postinst" "$staging/control/prerm"

		make_ipk "$staging" "$OUTDIR/rinadown-server_${VERSION}_${arch}.ipk"
	done
}

build_luci() {
	VERSION=$1 OUTDIR=$2
	mkdir -p "$OUTDIR"

	work=$(mktemp -d)
	trap 'rm -rf "$work"' EXIT

	staging="$work/pkg"
	mkdir -p "$staging/control" "$staging/data"
	cp -r "$SCRIPT_DIR/luci/." "$staging/data/"

	size=$(du -sk "$staging/data" | cut -f1)

	cat > "$staging/control/control" <<-EOF
	Package: luci-app-rinadown
	Version: $VERSION
	Depends: luci-base, rinadown-server
	Section: luci
	Architecture: all
	Installed-Size: ${size}k
	Maintainer: RinaDown <https://rinadown.zerx.dev>
	License: AGPL-3.0
	Description: LuCI support for RinaDown download manager.
	 Service configuration page and a shortcut to the RinaDown web interface.
	EOF

	cat > "$staging/control/postinst" <<-'EOF'
	#!/bin/sh
	[ -n "$IPKG_INSTROOT" ] || {
		rm -rf /tmp/luci-indexcache* /tmp/luci-modulecache 2>/dev/null
		/etc/init.d/rpcd reload 2>/dev/null
	}
	exit 0
	EOF
	chmod 755 "$staging/control/postinst"

	make_ipk "$staging" "$OUTDIR/luci-app-rinadown_${VERSION}_all.ipk"
}

cmd=${1:-}
case "$cmd" in
	server)
		[ $# -eq 5 ] || { echo "usage: $0 server <version> <binary> <arches> <outdir>" >&2; exit 2; }
		build_server "$2" "$3" "$4" "$5"
		;;
	luci)
		[ $# -eq 3 ] || { echo "usage: $0 luci <version> <outdir>" >&2; exit 2; }
		build_luci "$2" "$3"
		;;
	*)
		echo "usage: $0 {server|luci} ..." >&2
		exit 2
		;;
esac
