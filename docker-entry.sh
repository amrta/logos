#!/bin/sh
if [ ! -f /data/language.bin ]; then
  cp -r /app/seed/* /data/
fi
exec /usr/local/bin/logos
