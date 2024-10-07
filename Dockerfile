FROM stagex/binutils:sx2024.09.0@sha256:30a1bd110273894fe91c3a4a2103894f53eaac43cf12a035008a6982cb0e6908 AS binutils
FROM stagex/ca-certificates:sx2024.09.0@sha256:33787f1feb634be4232a6dfe77578c1a9b890ad82a2cf18c11dd44507b358803 AS ca-certificates
FROM stagex/gcc:sx2024.09.0@sha256:439bf36289ef036a934129d69dd6b4c196427e4f8e28bc1a3de5b9aab6e062f0 AS gcc
FROM stagex/zlib:sx2024.09.0@sha256:96b4100550760026065dac57148d99e20a03d17e5ee20d6b32cbacd61125dbb6 AS zlib
FROM stagex/llvm:sx2024.09.0@sha256:30517a41af648305afe6398af5b8c527d25545037df9d977018c657ba1b1708f AS llvm
FROM stagex/openssl:sx2024.09.0@sha256:2c1a9d8fcc6f52cb11a206f380b17d74c1079f04cbb08071a4176648b4df52c1 AS openssl
FROM stagex/eif_build:sx2024.09.0@sha256:291653f1ca528af48fd05858749c443300f6b24d2ffefa7f5a3a06c27c774566 AS eif_build
FROM stagex/gen_initramfs:sx2024.09.0@sha256:f5b9271cca6003e952cbbb9ef041ffa92ba328894f563d1d77942e6b5cdeac1a AS gen_initramfs
FROM stagex/libunwind:sx2024.09.0@sha256:97ee6068a8e8c9f1c74409f80681069c8051abb31f9559dedf0d0d562d3bfc82 AS libunwind
FROM stagex/rust:sx2024.09.0@sha256:b7c834268a81bfcc473246995c55b47fe18414cc553e3293b6294fde4e579163 AS rust
FROM stagex/musl:sx2024.09.0@sha256:ad351b875f26294562d21740a3ee51c23609f15e6f9f0310e0994179c4231e1d AS musl
FROM stagex/git:sx2024.09.0@sha256:29a02c423a4b55fa72cf2fce89f3bbabd1defea86d251bb2aea84c056340ab22 AS git
FROM stagex/pkgconf:sx2024.09.0@sha256:ba7fce4108b721e8bf1a0d993a5f9be9b65eceda8ba073fe7e8ebca2a31b1494 AS pkgconf
FROM stagex/busybox:sx2024.09.0@sha256:d34bfa56566aa72d605d6cbdc154de8330cf426cfea1bc4ba8013abcac594395 AS busybox
FROM stagex/linux-nitro:sx2024.03.0@sha256:073c4603686e3bdc0ed6755fee3203f6f6f1512e0ded09eaea8866b002b04264 AS linux-nitro

FROM scratch AS socat_base
ENV VERSION=1.8.0.0
ENV SRC_HASH=6010f4f311e5ebe0e63c77f78613d264253680006ac8979f52b0711a9a231e82
ENV SRC_FILE=socat-${VERSION}.tar.gz
ENV SRC_SITE=http://www.dest-unreach.org/socat/download/${SRC_FILE}

FROM socat_base AS socat_fetch
ADD --checksum=sha256:${SRC_HASH} ${SRC_SITE} ${SRC_FILE}

FROM socat_fetch AS socat_build
COPY --from=stagex/busybox . /
COPY --from=stagex/musl . /
COPY --from=stagex/gcc . /
COPY --from=stagex/binutils . /
COPY --from=stagex/make . /
COPY --from=stagex/linux-headers . /
RUN tar -xvf $SRC_FILE
WORKDIR /socat-${VERSION}
ENV SOURCE_DATE_EPOCH=1
RUN --network=none \
    LDFLAGS="-static" ./configure \
    --build=x86_64-unknown-linux-musl \
    --host=x86_64-unknown-linux-musl \
    --enable-static \
    --enable-vsock \
    --disable-shared \
    --prefix=/usr/ && \
    make -j"$(nproc)"

FROM socat_build AS socat_install
RUN --network=none make DESTDIR=/rootfs install

FROM stagex/filesystem AS socat_package
COPY --from=socat_install /rootfs/. /

FROM scratch AS net_tools_base
ENV VERSION=2.10
ENV SRC_HASH=b262435a5241e89bfa51c3cabd5133753952f7a7b7b93f32e08cb9d96f580d69
ENV SRC_FILE=net-tools-2.10.tar.xz
ENV SRC_SITE=https://downloads.sourceforge.net/project/net-tools/net-tools-2.10.tar.xz

FROM net_tools_base AS net_tools_fetch
ADD --checksum=sha256:${SRC_HASH} ${SRC_SITE} ${SRC_FILE}

FROM net_tools_fetch AS net_tools_build
COPY --from=stagex/busybox . /
COPY --from=stagex/musl . /
COPY --from=stagex/gcc . /
COPY --from=stagex/binutils . /
COPY --from=stagex/bash . /
COPY --from=stagex/make . /
COPY --from=stagex/linux-headers . /
RUN tar -xvf $SRC_FILE
WORKDIR /net-tools-${VERSION}
ENV CC="gcc -static"
ENV CFLAGS="-static"
ENV LDFLAGS="-static"
RUN export BINDIR='/' SBINDIR='/' && \ 
yes "" | make -j1                 && \
make DESTDIR=/rootfs -j1 install  && \
unset BINDIR SBINDIR

FROM stagex/filesystem AS net_tools_package
COPY --from=net_tools_build /rootfs/. /

FROM scratch AS base
ENV TARGET=x86_64-unknown-linux-musl
ENV RUSTFLAGS="-C target-feature=+crt-static"
ENV CARGOFLAGS="--locked --no-default-features --release --target ${TARGET}"
ENV OPENSSL_STATIC=true

COPY --from=busybox . /
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
COPY --from=gcc . /
COPY --from=linux-nitro /bzImage .
COPY --from=linux-nitro /nsm.ko .
COPY --from=linux-nitro /linux.config .
COPY --from=socat_package /usr/bin/socat .
COPY --from=socat_package /usr/bin/socat1 .
COPY --from=net_tools_package /ifconfig .
COPY --from=busybox /bin/udhcpc /udhcpc 

ADD . /

FROM base AS build
WORKDIR /src/init
RUN cargo build ${CARGOFLAGS}
WORKDIR /build_cpio
RUN cp /src/init/target/${TARGET}/release/init init
RUN cp /vm vm
ENV KBUILD_BUILD_TIMESTAMP=1
COPY <<-EOF initramfs.list
	file /init     init        0755 0 0
	file /nsm.ko   /nsm.ko     0755 0 0
	file /socat    /socat      0755 0 0
	file /socat1   /socat1     0755 0 0
	file /ifconfig /ifconfig   0755 0 0
	file /vm       /vm         0755 0 0
	file /udhcpc  /bin/udhcpc  0755 0 0
	dir  /run              	   0755 0 0
	dir  /tmp                  0755 0 0
	dir  /etc                  0755 0 0
	dir  /bin                  0755 0 0
	dir  /sbin                 0755 0 0
	dir  /proc                 0755 0 0
	dir  /sys                  0755 0 0
	dir  /usr                  0755 0 0
	dir  /usr/bin              0755 0 0
	dir  /usr/sbin             0755 0 0
	dir  /dev                  0755 0 0
	dir  /dev/shm              0755 0 0
	dir  /dev/pts              0755 0 0
	nod  /dev/console          0600 0 0 c 5 1
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

FROM base AS install
WORKDIR /rootfs
COPY --from=build /nitro.eif .
COPY --from=build /nitro.pcrs .

FROM scratch AS package
COPY --from=install /rootfs .