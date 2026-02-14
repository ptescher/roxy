FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Core tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    wget \
    git \
    build-essential \
    python3 \
    python3-pip \
    jq \
    dnsutils \
    iputils-ping \
    net-tools \
    iproute2 \
    && rm -rf /var/lib/apt/lists/*

# Node.js 22 (LTS)
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# kubectl
RUN curl -fsSL "https://dl.k8s.io/release/$(curl -Ls https://dl.k8s.io/release/stable.txt)/bin/linux/$(dpkg --print-architecture)/kubectl" \
    -o /usr/local/bin/kubectl \
    && chmod +x /usr/local/bin/kubectl

COPY entrypoint.sh /usr/local/bin/sandbox-entrypoint
RUN chmod +x /usr/local/bin/sandbox-entrypoint

WORKDIR /workspace

ENTRYPOINT ["sandbox-entrypoint"]
CMD ["/bin/bash"]
