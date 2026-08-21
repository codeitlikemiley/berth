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
        x11-xserver-utils \
        x11vnc \
        xdotool \
        xfonts-base \
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

# Sourced by /usr/bin/chromium so desktop files and PATH both get Docker flags.
# --no-sandbox is a Chromium argv, not a container privilege.
RUN printf '%s\n' \
    '# Guest Chromium has no user namespace / setuid sandbox.' \
    'if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && [ -f /tmp/berth-dbus.env ]; then . /tmp/berth-dbus.env; fi' \
    'export CHROMIUM_FLAGS="$CHROMIUM_FLAGS --no-sandbox --no-first-run --disable-sync --password-store=basic --disable-gpu"' \
    > /etc/chromium.d/berth-docker \
    && printf '%s\n' '#!/bin/sh' 'exec /usr/bin/chromium "$@"' \
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
      <action name="Execute"><command>/usr/bin/chromium</command></action>
    </item>
  </menu>
</openbox_menu>
EOF

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
COPY scripts/action.sh /usr/local/bin/action.sh
RUN chmod 755 /usr/local/bin/entrypoint.sh /usr/local/bin/action.sh \
    && ln -sf action.sh /usr/local/bin/driver \
    && printf '%s\n' \
        '#!/bin/bash' \
        'set -euo pipefail' \
        'test -f /tmp/berth-ready' \
        'xdpyinfo -display :99 >/dev/null' \
        "bash -c 'echo >/dev/tcp/127.0.0.1/6080'" \
        > /usr/local/bin/berth-health \
    && chmod 755 /usr/local/bin/berth-health \
    && cp /etc/xdg/openbox/menu.xml /home/berth/.config/openbox/menu.xml \
    && chown berth:berth /home/berth/.config/openbox/menu.xml

# Viewer is noVNC on 6080; raw VNC is localhost-only inside the guest.
EXPOSE 6080
HEALTHCHECK --interval=10s --timeout=3s --start-period=12s --retries=5 \
    CMD ["/usr/local/bin/berth-health"]

USER berth
WORKDIR /workspace
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/entrypoint.sh"]
