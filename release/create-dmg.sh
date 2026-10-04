#!/bin/sh
# Builds Keyternity.dmg from Keyternity.app. Laying out the Finder window needs
# a desktop session, so CI runners (CI is set) skip that step.
rm -f Keyternity.dmg
create-dmg \
  --background dmg_bg.png \
  --window-pos 200 200 \
  --window-size 600 300 \
  --icon-size 100 \
  --icon Keyternity.app 0 125 \
  --hide-extension "Keyternity.app" \
  --app-drop-link 350 125 \
  ${CI:+--skip-jenkins} \
  Keyternity.dmg Keyternity.app
