#!/usr/bin/env python3
"""Builds a generated zmk-config directory with the real ZMK toolchain.

Mirrors what ZMK's build-user-config workflow does for a user's repository,
so a config that builds here builds for users. Run inside the
zmkfirmware/zmk-build-arm container: `ci/build-fixture.py fixtures/<board>`.
"""

import os
import subprocess
import sys

import yaml


def run(*command, cwd):
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, check=True)


def main():
    root = os.path.abspath(sys.argv[1])
    config = os.path.join(root, "config")
    if not os.path.isdir(os.path.join(root, ".west")):
        run("west", "init", "-l", config, cwd=root)
    run("west", "update", "--fetch-opt=--filter=tree:0", cwd=root)
    run("west", "zephyr-export", cwd=root)

    with open(os.path.join(root, "build.yaml")) as matrix:
        targets = yaml.safe_load(matrix)["include"]
    for target in targets:
        name = target["artifact-name"]
        command = ["west", "build", "-p", "-s", "zmk/app", "-d", f"build/{name}", "-b", target["board"]]
        if "snippet" in target:
            command += ["-S", target["snippet"]]
        command += ["--", f"-DZMK_CONFIG={config}"]
        if "shield" in target:
            command.append(f"-DSHIELD={target['shield']}")
        command += target.get("cmake-args", "").split()
        run(*command, cwd=root)
        firmware = os.path.join(root, "build", name, "zephyr", "zmk.uf2")
        if not os.path.isfile(firmware):
            sys.exit(f"{name}: the build finished but produced no zmk.uf2")
        print(f"{name}: {os.path.getsize(firmware)} bytes", flush=True)


if __name__ == "__main__":
    main()
