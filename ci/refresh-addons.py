#!/usr/bin/env python3
"""Refreshes the generated half of the ZMK add-on catalog.

Reads crates/kc-zmk/catalog/addons.toml (curated by hand) and rewrites
addons-meta.json beside it with, for each add-on, the commit its `ref`
points to, its stars, the date of its last push and whether it is archived.
It also lists repositories with GitHub's `zmk-module` topic that the catalog
does not have, as candidates for a person to review: nothing is added to
the catalog automatically, because descriptions and compatibility notes
have to be written and checked.

Needs a GitHub token in GH_TOKEN or GITHUB_TOKEN: the run makes a few
dozen API requests, more than GitHub allows without one.
"""

import datetime
import json
import os
import sys
import tomllib
import urllib.error
import urllib.request

CATALOG = "crates/kc-zmk/catalog/addons.toml"
META = "crates/kc-zmk/catalog/addons-meta.json"
API = "https://api.github.com"
# Candidates below this many stars are mostly personal experiments.
MIN_STARS = 15


def get(path):
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if not token:
        sys.exit("set GH_TOKEN or GITHUB_TOKEN")
    request = urllib.request.Request(
        API + path,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "User-Agent": "keyboard-curator-addon-refresh",
        },
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def repository(url):
    return url.removeprefix("https://github.com/").strip("/")


def main():
    with open(CATALOG, "rb") as file:
        catalog = tomllib.load(file)
    try:
        with open(META) as file:
            previous = json.load(file)
    except FileNotFoundError:
        previous = {}

    addons = {}
    known = set()
    for addon in catalog.get("addon", []):
        repo = repository(addon["url"])
        known.add(repo.lower())
        try:
            info = get(f"/repos/{repo}")
            commit = get(f"/repos/{repo}/commits/{addon['ref']}")
        except urllib.error.HTTPError as error:
            # Keep what was known, so that one missing repository does not
            # unpin an add-on.
            print(f"warning: {repo}: {error}", file=sys.stderr)
            if addon["id"] in previous.get("addons", {}):
                addons[addon["id"]] = previous["addons"][addon["id"]]
            continue
        addons[addon["id"]] = {
            "revision": commit["sha"],
            "stars": info["stargazers_count"],
            "pushed": info["pushed_at"][:10],
            "archived": info["archived"],
        }

    ignored = {name.lower() for name in catalog.get("ignore", [])}
    found = get("/search/repositories?q=topic:zmk-module&sort=stars&order=desc&per_page=100")
    candidates = [
        {
            "name": item["full_name"],
            "url": item["html_url"],
            "description": item["description"] or "",
            "stars": item["stargazers_count"],
        }
        for item in found["items"]
        if item["full_name"].lower() not in known
        and item["full_name"].lower() not in ignored
        and not item["archived"]
        and item["stargazers_count"] >= MIN_STARS
    ]

    meta = {"format": 1, "addons": addons, "candidates": candidates}
    # The date only moves when something else did, so that an unchanged
    # week makes no pull request.
    unchanged = {k: previous.get(k) for k in meta} == meta
    meta["checked"] = (
        previous.get("checked") if unchanged else datetime.date.today().isoformat()
    )
    with open(META, "w") as file:
        json.dump(meta, file, indent=2, sort_keys=True)
        file.write("\n")
    print(f"{len(addons)} add-ons, {len(candidates)} candidates, {'no change' if unchanged else 'updated'}")


if __name__ == "__main__":
    main()
