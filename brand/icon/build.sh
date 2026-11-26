#!/usr/bin/env bash
# Rebuild every platform's icon files from the super-voice-icon skill's
# layers, then copy the Linux set into the app's resources.
#
# Needs: Python 3 with Pillow, shapely and scour; rsvg-convert (librsvg);
# Node with Playwright and its Chromium. See the skill's SKILL.md.
set -euo pipefail
cd "$(dirname "$0")/../.."

python3 brand/icon/master/build.py
python3 brand/icon/macos/tools/build.py
python3 brand/icon/linux/build.py
python3 brand/icon/windows/src/build.py

# The app installs the Linux icon theme from its own resources.
cp -r brand/icon/linux/hicolor/. super-voice-gpui/resources/icons/hicolor/
find brand/icon -name __pycache__ -prune -exec rm -rf {} +
echo "✓ Rebuilt the master, macOS, Linux and Windows icons"
