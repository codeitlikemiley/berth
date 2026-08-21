# berthOS Linux guest: Xvfb desktop. Does not drive the host.
# openbox+tint2 instead of xfce4 to stay well under ~1.5 GB.
FROM debian:bookworm-slim

ENV DISPLAY=:99 \
    WIDTH=1280 \
    HEIGHT=800 \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    HOME=/home/berth \
    USER=berth \
    XDG_RUNTIME_DIR=/tmp/runtime-berth

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates \
        chromium \
        curl \
        dbus-x11 \
        fonts-liberation \
        git \
        imagemagick \
        jq \
        novnc \
        openbox \
        procps \
        python3 \
        tini \
        tint2 \
        websockify \
        x11-utils \
        x11vnc \
        xdotool \
        xterm \
        xvfb \
    && rm -rf /var/lib/apt/lists/* \
    && ln -sf /usr/share/novnc/vnc.html /usr/share/novnc/index.html \
    && groupadd -g 1000 berth \
    && useradd -m -u 1000 -g 1000 -s /bin/bash berth \
    && mkdir -p /workspace \
        /home/berth/.config/openbox \
        /home/berth/.config/chromium \
        /etc/xdg/openbox \
    && touch "/home/berth/.config/chromium/First Run" \
    && chown -R berth:berth /home/berth /workspace

# Debian chromium dies in Docker without these flags.
RUN printf '%s\n' \
    '#!/bin/sh' \
    'exec /usr/bin/chromium --no-sandbox --disable-dev-shm-usage --disable-gpu --disable-software-rasterizer --no-first-run --disable-sync --password-store=basic --disable-features=TranslateUI "$@"' \
    > /usr/local/bin/chromium \
    && chmod +x /usr/local/bin/chromium \
    && ln -sf chromium /usr/local/bin/berth-chromium

RUN cat > /etc/xdg/openbox/menu.xml <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<openbox_menu xmlns="http://openbox.org/3.4/menu">
  <menu id="root-menu" label="berthOS">
    <item label="Terminal">
      <action name="Execute"><command>xterm</command></action>
    </item>
    <item label="Chromium">
      <action name="Execute"><command>/usr/local/bin/chromium</command></action>
    </item>
  </menu>
</openbox_menu>
EOF
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
COPY scripts/action.sh /usr/local/bin/action.sh
RUN chmod 755 /usr/local/bin/entrypoint.sh /usr/local/bin/action.sh \
    && ln -sf action.sh /usr/local/bin/driver \
    && cp /etc/xdg/openbox/menu.xml /home/berth/.config/openbox/menu.xml \
    && chown berth:berth /home/berth/.config/openbox/menu.xml

EXPOSE 5900 6080
HEALTHCHECK --interval=10s --timeout=3s --start-period=12s --retries=5 \
    CMD ["test", "-f", "/tmp/berth-ready"]

USER berth
WORKDIR /workspace
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/entrypoint.sh"]
