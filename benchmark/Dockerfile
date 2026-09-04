FROM rust:1-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    build-essential \
    ca-certificates \
    clang \
    cmake \
    git \
    libboost-dev \
    libevent-dev \
    libsqlite3-dev \
    ninja-build \
    pkg-config \
    python3 \
    python3-matplotlib \
    sqlite3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/bcore-mutation
COPY . /opt/bcore-mutation
RUN cargo install --path /opt/bcore-mutation

ENTRYPOINT ["python3", "/opt/bcore-mutation/benchmark/benchmark_parallel.py"]

