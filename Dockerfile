FROM stagex/rust@sha256:e7a7a152ddf91ba4f2d6e426867c54ab43b76eef3f2a97dd0c3d9234090f3ce8 as rust
FROM stagex/bash@sha256:39c6d22701e58c79548cf0601e72f85bb07bf30608827540e74db30220802430 as bash
FROM stagex/coreutils@sha256:85341b2055493ff8bf3d90c9d4e7a5993e4dd7a1d11a06854f23e0434bb4abaa as coreutils
FROM stagex/findutils@sha256:d0d30ce5d176fe2e40e93f707220ae6f54788ff14972005d1a51961c17f5294b as findutils
FROM stagex/grep@sha256:565d7cc8257d45f19326b3ecbbc2dd3096b4a228977c91f4ed07a265faeb8b05 as grep
FROM stagex/musl@sha256:27ca6026619beae07a1e7096caa7ac41b1403f5c1839ed4ff79b5aee3c409cec as musl
FROM stagex/libunwind@sha256:422fe0a108d9f1253dd9694ce432aa195d49a3b60b1d977aa4e94024c7ac52bf as libunwind
FROM stagex/openssl@sha256:f4e218dba1167008456899c5f19d9e1a1be17d4fc6fb6bb84d41b8eb477fd402 as openssl
FROM stagex/zlib@sha256:d5df909418ef436e3dd23af397ba2b202bd72f45c81b0e161b507adc9e3e9b9c as zlib
FROM stagex/ca-certificates@sha256:70c5136051c748fff0d1399101d082ecc204c1eb29d93da094ccf0d25f341121 as ca-certificates
FROM stagex/binutils@sha256:9cc26e56cdfce106108a0f4c416a27967060d8d07c4da0cbc0e14fa87f7b1dfa as binutils
FROM stagex/pkgconf@sha256:36fc4ed10a6e044d068aa7316e72588dbd365be4eb0271a84cf632521dbd8a09 as pkgconf
FROM stagex/git@sha256:3a2853fa2fa725f7f02565e24f508912b33223e49bed915e55a5d3f85548d190 as git
FROM stagex/gen_initramfs@sha256:66b9b1757dc6f66495d205417d14b79ab25f5b107c5caf609e4d4b9967b6ca6e as gen_initramfs
FROM stagex/eif_build@sha256:561ac95d02f1a5caf1d600cd2dbf487d1bb63450de0af2b528a9b657c66c12a8 as eif_build
FROM stagex/llvm@sha256:9dfc53795c89295da52719959f96df9122e0b921da6283c7bd7a582749545b1d as llvm
FROM stagex/file@sha256:8ce66c0574777bca83c8297b74372e0be7a6cc5d2b7e21061391726ad6d6d406 as file
FROM stagex/gcc@sha256:bb550daddcf95acdce9999e359e3ffb1c497916aea41bdd0cae1d6a5a908b4b9 as gcc
FROM stagex/linux-nitro@sha256:dd38b784ea9f8f0757e549194d078cccde9d6aed46915df2be9086880693fb17 as linux-nitro

FROM scratch as base
ENV TARGET=x86_64-unknown-linux-musl
ENV RUSTFLAGS="-C target-feature=+crt-static"
ENV CARGOFLAGS="--locked --no-default-features --release --target ${TARGET}"
ENV OPENSSL_STATIC=true
COPY --from=bash /bin/bash /bin/sh
COPY --from=coreutils . /
COPY --from=findutils . /
COPY --from=grep . /
COPY --from=musl . /
COPY --from=libunwind . /
COPY --from=openssl . /
COPY --from=zlib . /
COPY --from=ca-certificates . /
COPY --from=binutils . /
COPY --from=pkgconf . /
COPY --from=git . /
COPY --from=rust . /
COPY --from=gen_initramfs . /
COPY --from=eif_build . /
COPY --from=llvm . /
COPY --from=file . /
COPY --from=gcc . /
COPY --from=gcc /usr/lib64/* /usr/lib/
COPY --from=linux-nitro /bzImage .
COPY --from=linux-nitro /nsm.ko .
COPY --from=linux-nitro /linux.config .
RUN mkdir /tmp
ADD . /src

FROM base as build
RUN <<-EOF
	set -eux
	env -C /src/src/init cargo build ${CARGOFLAGS}
	cp /src/src/init/target/${TARGET}/release/init /
	file /init | grep "static-pie"
EOF
WORKDIR /build_cpio
ENV KBUILD_BUILD_TIMESTAMP=1
COPY <<-EOF initramfs.list
	file /init     init    0755 0 0
	file /nsm.ko   /nsm.ko 0755 0 0
	dir  /run              0755 0 0
	dir  /tmp              0755 0 0
	dir  /etc              0755 0 0
	dir  /bin              0755 0 0
	dir  /sbin             0755 0 0
	dir  /proc             0755 0 0
	dir  /sys              0755 0 0
	dir  /usr              0755 0 0
	dir  /usr/bin          0755 0 0
	dir  /usr/sbin         0755 0 0
	dir  /dev              0755 0 0
	dir  /dev/shm          0755 0 0
	dir  /dev/pts          0755 0 0
	nod  /dev/console      0600 0 0 c 5 1
EOF
RUN <<-EOF
	find . -exec touch -hcd "@0" "{}" +
	gen_init_cpio -t 1 initramfs.list > rootfs.cpio
	touch -hcd "@0" rootfs.cpio
EOF
WORKDIR /build_eif
RUN eif_build \
	--kernel /bzImage \
	--kernel_config /linux.config \
	--ramdisk /build_cpio/rootfs.cpio \
	--pcrs_output /nitro.pcrs \
	--output /nitro.eif \
	--cmdline 'reboot=k initrd=0x2000000,3228672 root=/dev/ram0 panic=1 pci=off nomodules console=ttyS0 i8042.noaux i8042.nomux i8042.nopnp i8042.dumbkbd'

FROM base as install
WORKDIR /rootfs
COPY --from=build /nitro.eif .
COPY --from=build /nitro.pcrs .
RUN find . -exec touch -hcd "@0" "{}" +

FROM scratch as package
COPY --from=install /rootfs .
