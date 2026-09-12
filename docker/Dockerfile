# ==========================================
# Phase 1: Build Phase
# ==========================================
FROM rust:1.95-slim-bookworm AS builder

# 安装构建依赖，包括 pkg-config 和 openssl，因为依赖库 reqwest 等可能需要它们
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/iota-sympantos

# 复制整个项目源码
COPY . .

# 利用 Cargo 构建发布版本二进制
# 由于 rusqlite 开启了 "bundled" 特性，这里不需要在构建系统里安装 libsqlite3-dev
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/iota-sympantos/target \
    cargo build --release --package iota-cli

# ==========================================
# Phase 2: Runtime Phase
# ==========================================
FROM debian:bookworm-slim

# 设置非交互式前端
ENV DEBIAN_FRONTEND=noninteractive

# 安装运行所需的依赖：
# - ca-certificates: 提供 HTTPS 根证书支持
# - git: 供 AI 编程后端进行代码的版本控制和修改
# - curl: 用于健康检查或网络访问
# - sqlite3: 调试与排查数据库文件
# - nodejs/npm: Iota 各后端的核心运行环境（claude-code, codex, gemini 等通过 npx 运行）
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    curl \
    sqlite3 \
    nodejs \
    npm \
    && rm -rf /var/lib/apt/lists/*

# 创建非 root 账号并规划家目录
RUN groupadd -g 1000 iota && \
    useradd -u 1000 -g iota -m -s /bin/bash iota

# 预设环境，允许 npx 和 npm 在 iota 账户下免权限问题执行
ENV NPM_CONFIG_PREFIX=/home/iota/.npm-global
ENV PATH=$PATH:/home/iota/.npm-global/bin

# 创建程序工作目录及 iota 配置目录，并赋予权限
RUN mkdir -p /workspace /home/iota/.i6 /home/iota/.npm-global && \
    chown -R iota:iota /workspace /home/iota/.i6 /home/iota/.npm-global

# 从编译阶段复制编译好的二进制文件
COPY --from=builder /usr/src/iota-sympantos/target/release/iota /usr/local/bin/iota

# 复制 entrypoint.sh 脚本
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# 预拷贝 nimia.yaml.template 模板，供 entrypoint 初始化使用
COPY nimia.yaml.template /home/iota/.i6/nimia.yaml.template
RUN chown iota:iota /home/iota/.i6/nimia.yaml.template

# 切换为安全的用户运行容器
USER iota
WORKDIR /workspace

# 声明 iota 的持久化配置卷，确保 SQLite 数据库和配置能够保存
VOLUME ["/home/iota/.i6", "/workspace"]

# 默认导出 Iota Daemon 网络服务端口
EXPOSE 47661

# 预配置默认的环境变量
ENV IOTA_DAEMON_ADDR=0.0.0.0:47661
ENV OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["daemon"]
