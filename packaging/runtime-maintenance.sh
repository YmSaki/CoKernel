#!/bin/sh
# Invoked only by the current-user Windows installer, as root in its verified distro.
set -eu
[ "$(id -u)" = 0 ] || exit 71
work=/opt/.cokernel-maintenance
case "$1" in
prepare)
  /opt/cokernel/bin/cokernel-bootstrap inspect >/dev/null
  [ ! -e "$work" ] || { echo 'CK-UPDATE-PENDING: recover previous update first' >&2; exit 72; }
  mkdir -m 700 "$work"
  mkdir -m 700 "$work/new" "$work/old"
  ;;
apply)
  [ -d "$work/new/opt/cokernel" ] && [ ! -e "$work/backed-up" ]
  cp -a /opt/cokernel "$work/old/cokernel"
  cp -a /etc/cokernel-validation.json "$work/old/manifest"
  cp -a /etc/ssl/certs/ca-certificates.crt "$work/old/ca"
  cp -a /etc/ssl/cert.pem "$work/old/python-ca"
  sync
  touch "$work/backed-up"
  sync
  # Existing Project/state/cache trees are never extracted or moved.
  mv /opt/cokernel "$work/displaced"
  mv "$work/new/opt/cokernel" /opt/cokernel
  cp -p "$work/new/etc/cokernel-validation.json" /etc/cokernel-validation.json
  cp -p "$work/new/etc/ssl/certs/ca-certificates.crt" /etc/ssl/certs/ca-certificates.crt
  cp -p "$work/new/etc/ssl/cert.pem" /etc/ssl/cert.pem
  /opt/cokernel/bin/cokernel-bootstrap finalize "$2"
  sync
  ;;
rollback)
  if [ -f "$work/backed-up" ]; then
    # Keep immutable backup until every restore and verification succeeds.
    rm -rf "$work/restore"
    cp -a "$work/old/cokernel" "$work/restore"
    rm -rf /opt/cokernel
    mv "$work/restore" /opt/cokernel
    cp -p "$work/old/manifest" /etc/cokernel-validation.json
    cp -p "$work/old/ca" /etc/ssl/certs/ca-certificates.crt
    cp -p "$work/old/python-ca" /etc/ssl/cert.pem
    /opt/cokernel/bin/cokernel-bootstrap finalize "$2"
  fi
  rm -rf "$work"
  sync
  ;;
commit)
  /opt/cokernel/bin/cokernel-bootstrap inspect >/dev/null
  rm -rf "$work"
  sync
  ;;
*) exit 64 ;;
esac
