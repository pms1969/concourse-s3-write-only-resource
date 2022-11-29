FROM rust:1 as builder

RUN apt update \
    && apt install -y openssl \
    && apt-get install -y --no-install-recommends ca-certificates \
    && update-ca-certificates \
    && apt-get install python3-pip rsync -y \
    && pip3 install git+https://github.com/larsks/dockerize

WORKDIR /usr/src/resource
COPY Cargo.toml ./

# creates dummy main to compile dependencies against.
# this prevents local builds from having to build everything in the event
# there are no changes to the dependencies.
RUN mkdir src \
    && echo "fn main() {print!(\"Dummy Resource\");}" > src/main.rs

# build dependencies
RUN cargo build --release

# build Resourece
COPY src ./src
RUN cargo build --release \
    && cargo install --path . --target-dir /tmp/bin \
    && strip /tmp/bin/release/resource

RUN mkdir -p /opt/resource \
    && cp /tmp/bin/release/resource /opt/resource/concourse-resource-s3-write-only \
    && ln -s /opt/resource/concourse-resource-s3-write-only /opt/resource/check  \
    && ln -s /opt/resource/concourse-resource-s3-write-only /opt/resource/out \
    && ln -s /opt/resource/concourse-resource-s3-write-only /opt/resource/in \
    && dockerize -n -o /tmp/app -a /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt /usr/bin/strace /opt/resource/concourse-resource-s3-write-only /opt/resource/check /opt/resource/in /opt/resource/out

# * Create the release image.
FROM scratch
COPY --from=builder /tmp/app/ /
ENV RUST_LOG=debug RUST_BACTRACE=1


USER 1714
