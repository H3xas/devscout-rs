#!/usr/bin/env python3
"""Unit tests for tools/dotnet-snapshots.py.

The validator is exercised against synthetic roots: a stub `dotnet` host that answers the three
version queries the way an install would, a reference-pack tree and a workload-manifest tree whose
bytes a test can then corrupt, add to, remove from or replace with a symlink. Nothing here runs a
real SDK, so the checks stay fast and need no .NET install; the end-to-end regeneration is the tool
itself, run from a validated root.

Run with: python3 -m unittest discover -s tools -p test_dotnet_snapshots.py
"""

import copy
import importlib.util
import io
import json
import os
import shutil
import stat
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest import mock

TOOLS = Path(__file__).resolve().parent

_spec = importlib.util.spec_from_file_location("dotnet_snapshots", TOOLS / "dotnet-snapshots.py")
snapshots = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(snapshots)

HOST_STUB = """#!/bin/sh
# The version queries answer from the root's own sdk/ directory, the way a real host does, so a
# root a test has given a second SDK really does report two.
root=$(dirname "$0")
case "$1" in
  --list-sdks)
    for dir in "$root"/sdk/*/; do
      printf '%s [%s]\\n' "$(basename "$dir")" "$root/sdk"
    done
    ;;
  --version)
    highest=$(basename "$(ls -d "$root"/sdk/*/ | sort | tail -n 1)")
    printf '%s\\n' "$highest"
    ;;
  msbuild) printf '%s\\n' "17.14.21.41108" ;;
  *) exit 1 ;;
esac
"""

SDK_VERSION = "9.0.305"
PACK_ID = "Microsoft.NETCore.App.Ref"
PACK_VERSION = "9.0.9"
PACK_TFM = "net9.0"
BAND = "9.0.100"


class SyntheticRoot:
    """A disposable SDK root a test shapes one detail at a time."""

    def __init__(self, base, sdk_version=SDK_VERSION, extra_sdk=None):
        self.path = Path(base) / "dotnet-root"
        (self.path / "sdk" / sdk_version).mkdir(parents=True)
        if extra_sdk:
            (self.path / "sdk" / extra_sdk).mkdir()
        self.pack = self.path / "packs" / PACK_ID / PACK_VERSION / "ref" / PACK_TFM
        self.pack.mkdir(parents=True)
        self.manifests = self.path / "sdk-manifests" / BAND
        self.manifests.mkdir(parents=True)
        for name, body in (
            ("System.Runtime.dll", "reference-bytes"),
            ("System.Console.dll", "more-reference-bytes"),
        ):
            (self.pack / name).write_text(body, encoding="utf-8")
        for name in ("WorkloadManifest.json", "WorkloadManifest.targets"):
            (self.manifests / "microsoft.net.sdk.android" / name).parent.mkdir(
                parents=True, exist_ok=True
            )
            (self.manifests / "microsoft.net.sdk.android" / name).write_text(
                f"{name} body", encoding="utf-8"
            )
        host = self.path / "dotnet"
        host.write_text(HOST_STUB, encoding="utf-8")
        host.chmod(host.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    def pin(self):
        pack_count, pack_digest = snapshots.tree_inventory(self.pack, "the reference pack")
        manifest_count, manifest_digest = snapshots.tree_inventory(
            self.manifests, "the workload manifests"
        )
        return {
            "manifestVersion": snapshots.MANIFEST_VERSION,
            "sdkVersion": SDK_VERSION,
            "referencePack": {
                "id": PACK_ID,
                "version": PACK_VERSION,
                "targetFramework": PACK_TFM,
                "fileCount": pack_count,
                "treeDigest": pack_digest,
            },
            "workloadManifests": {
                "band": BAND,
                "fileCount": manifest_count,
                "treeDigest": manifest_digest,
            },
            "archives": {
                "osx-arm64": {
                    "name": "dotnet-sdk-9.0.305-osx-arm64.tar.gz",
                    "url": "https://example.invalid/osx-arm64.tar.gz",
                    "sha512": "a" * 128,
                },
                "linux-x64": {
                    "name": "dotnet-sdk-9.0.305-linux-x64.tar.gz",
                    "url": "https://example.invalid/linux-x64.tar.gz",
                    "sha512": "b" * 128,
                },
            },
        }


class SyntheticRootCase(unittest.TestCase):
    def setUp(self):
        self._dir = tempfile.TemporaryDirectory(prefix="dotnet-snapshots-test-")
        self.addCleanup(self._dir.cleanup)
        self.root = SyntheticRoot(self._dir.name)
        self.pin = self.root.pin()

    def validate(self, pin=None):
        return snapshots.validate_root(self.root.path, pin or self.pin)


class ValidRootTests(SyntheticRootCase):
    def test_a_dedicated_root_matching_the_pin_is_accepted(self):
        host = self.validate()
        self.assertEqual(host, self.root.path / "dotnet")

    def test_relative_root_is_refused(self):
        with self.assertRaises(snapshots.Refusal) as raised:
            snapshots.validate_root(Path("dotnet-root"), self.pin)
        self.assertIn("absolute", str(raised.exception))

    def test_missing_root_is_refused(self):
        with self.assertRaises(snapshots.Refusal) as raised:
            snapshots.validate_root(self.root.path / "absent", self.pin)
        self.assertIn("not a directory", str(raised.exception))

    def test_root_without_a_host_is_refused(self):
        (self.root.path / "dotnet").unlink()
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("no dotnet host", str(raised.exception))

    def test_a_second_sdk_in_the_root_is_refused(self):
        (self.root.path / "sdk" / "10.0.302").mkdir()
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("exactly one SDK", str(raised.exception))

    def test_a_root_resolving_another_version_is_refused(self):
        other = SyntheticRoot(tempfile.mkdtemp(dir=self._dir.name), sdk_version="9.0.304")
        with self.assertRaises(snapshots.Refusal) as raised:
            snapshots.validate_root(other.path, self.pin)
        self.assertIn("9.0.304", str(raised.exception))

    def test_an_unreadable_listing_is_refused(self):
        (self.root.path / "dotnet").write_text("#!/bin/sh\necho 'not a listing line'\n")
        (self.root.path / "dotnet").chmod(0o755)
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("cannot read", str(raised.exception))


class InputDriftTests(SyntheticRootCase):
    def test_changed_reference_bytes_are_refused(self):
        (self.root.pack / "System.Runtime.dll").write_text("tampered", encoding="utf-8")
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("reference pack", str(raised.exception))

    def test_an_extra_reference_file_is_refused(self):
        (self.root.pack / "System.Extra.dll").write_text("extra", encoding="utf-8")
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("reference pack", str(raised.exception))

    def test_a_removed_reference_file_is_refused(self):
        (self.root.pack / "System.Console.dll").unlink()
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("reference pack", str(raised.exception))

    def test_a_missing_reference_tree_is_refused(self):
        shutil.rmtree(self.root.path / "packs")
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("missing", str(raised.exception))

    def test_changed_workload_manifest_bytes_are_refused(self):
        targets = self.root.manifests / "microsoft.net.sdk.android" / "WorkloadManifest.targets"
        targets.write_text("tampered", encoding="utf-8")
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("workload manifests", str(raised.exception))

    def test_a_workload_manifest_added_beside_the_pin_is_refused(self):
        extra = self.root.manifests / "microsoft.net.sdk.extra"
        extra.mkdir()
        (extra / "WorkloadManifest.json").write_text("{}", encoding="utf-8")
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("workload manifests", str(raised.exception))

    def test_a_symlinked_reference_file_is_refused(self):
        real = self.root.pack / "System.Runtime.dll"
        link = self.root.pack / "System.Linked.dll"
        link.symlink_to(real)
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("symlink", str(raised.exception))

    def test_a_symlinked_directory_is_refused(self):
        (self.root.manifests / "escaping").symlink_to(self.root.path / "sdk-manifests")
        with self.assertRaises(snapshots.Refusal) as raised:
            self.validate()
        self.assertIn("symlinked directory", str(raised.exception))


class InventoryTests(SyntheticRootCase):
    def test_the_digest_is_independent_of_creation_order(self):
        first_count, first_digest = snapshots.tree_inventory(self.root.pack, "the reference pack")
        (self.root.pack / "System.Runtime.dll").unlink()
        (self.root.pack / "System.Runtime.dll").write_text("reference-bytes", encoding="utf-8")
        second_count, second_digest = snapshots.tree_inventory(self.root.pack, "the reference pack")
        self.assertEqual(first_count, second_count)
        self.assertEqual(first_digest, second_digest)

    def test_an_empty_tree_is_refused(self):
        empty = Path(self._dir.name) / "empty"
        empty.mkdir()
        with self.assertRaises(snapshots.Refusal) as raised:
            snapshots.tree_inventory(empty, "the empty tree")
        self.assertIn("empty", str(raised.exception))

    def test_a_renamed_path_changes_the_digest(self):
        _, before = snapshots.tree_inventory(self.root.pack, "the reference pack")
        (self.root.pack / "System.Console.dll").rename(self.root.pack / "System.Console2.dll")
        _, after = snapshots.tree_inventory(self.root.pack, "the reference pack")
        self.assertNotEqual(before, after)


class DeclarationTests(unittest.TestCase):
    def setUp(self):
        self._dir = tempfile.TemporaryDirectory(prefix="dotnet-snapshots-pin-")
        self.addCleanup(self._dir.cleanup)
        self.pin_path = Path(self._dir.name) / "pin.json"
        self.root = SyntheticRoot(self._dir.name)
        self.pin = self.root.pin()

    def write(self, value):
        self.pin_path.write_text(json.dumps(value), encoding="utf-8")

    def load(self):
        return snapshots.load_pin(self.pin_path)

    def assert_refused(self, pin, needle):
        self.write(pin)
        with self.assertRaises(snapshots.Refusal) as raised:
            self.load()
        self.assertIn(needle, str(raised.exception))

    def test_the_shipped_pin_is_readable(self):
        pin = snapshots.load_pin(snapshots.MANIFEST_PATH)
        self.assertEqual(pin["sdkVersion"], SDK_VERSION)
        self.assertEqual(sorted(pin["archives"]), ["linux-x64", "osx-arm64"])

    def test_a_well_formed_pin_loads(self):
        self.write(self.pin)
        self.assertEqual(self.load()["referencePack"]["fileCount"], self.pin["referencePack"]["fileCount"])

    def test_unreadable_json_is_refused(self):
        self.pin_path.write_text("{not json", encoding="utf-8")
        with self.assertRaises(snapshots.Refusal) as raised:
            self.load()
        self.assertIn("not readable JSON", str(raised.exception))

    def test_a_duplicate_key_is_refused(self):
        body = json.dumps(self.pin)
        self.pin_path.write_text(
            body[:-1] + ', "sdkVersion": "9.0.305"}', encoding="utf-8"
        )
        with self.assertRaises(snapshots.Refusal) as raised:
            self.load()
        self.assertIn("declares", str(raised.exception))

    def test_an_unknown_field_is_refused(self):
        pin = copy.deepcopy(self.pin)
        pin["sdkRoot"] = "/somewhere"
        self.assert_refused(pin, "unknown field")

    def test_a_missing_field_is_refused(self):
        pin = copy.deepcopy(self.pin)
        del pin["workloadManifests"]
        self.assert_refused(pin, "missing")

    def test_another_manifest_version_is_refused(self):
        pin = copy.deepcopy(self.pin)
        pin["manifestVersion"] = snapshots.MANIFEST_VERSION + 1
        self.assert_refused(pin, "manifestVersion")

    def test_a_malformed_sdk_version_is_refused(self):
        pin = copy.deepcopy(self.pin)
        pin["sdkVersion"] = "9.0"
        self.assert_refused(pin, "three-part version")

    def test_a_malformed_pack_digest_is_refused(self):
        pin = copy.deepcopy(self.pin)
        pin["referencePack"]["treeDigest"] = "sha256:short"
        self.assert_refused(pin, "digest")

    def test_a_malformed_archive_digest_is_refused(self):
        pin = copy.deepcopy(self.pin)
        pin["archives"]["osx-arm64"]["sha512"] = "A" * 128
        self.assert_refused(pin, "digest")

    def test_a_non_positive_file_count_is_refused(self):
        pin = copy.deepcopy(self.pin)
        pin["workloadManifests"]["fileCount"] = 0
        self.assert_refused(pin, "positive integer")

    def test_a_non_object_archives_table_is_refused(self):
        pin = copy.deepcopy(self.pin)
        pin["archives"] = []
        self.assert_refused(pin, "JSON object")

    def test_an_empty_archives_table_is_refused(self):
        pin = copy.deepcopy(self.pin)
        pin["archives"] = {}
        self.assert_refused(pin, "empty")

    def test_an_archive_without_a_url_is_refused(self):
        pin = copy.deepcopy(self.pin)
        del pin["archives"]["linux-x64"]["url"]
        self.assert_refused(pin, "missing")


class ChildEnvironmentTests(unittest.TestCase):
    def test_the_supplied_root_selects_and_leads_the_path(self):
        environment = snapshots.child_environment(Path("/pinned/root"))

        self.assertEqual(environment["DOTNET_ROOT"], "/pinned/root")
        self.assertTrue(environment["PATH"].startswith("/pinned/root" + os.pathsep))
        self.assertEqual(environment["DOTNET_MULTILEVEL_LOOKUP"], "0")

    def test_an_inherited_sdk_selection_never_reaches_a_child(self):
        decoys = {variable: f"/launcher/{variable}" for variable in snapshots.SDK_SELECTION_VARIABLES}
        with mock.patch.dict(os.environ, decoys):
            environment = snapshots.child_environment(Path("/pinned/root"))

        for variable in snapshots.SDK_SELECTION_VARIABLES:
            self.assertNotIn(variable, environment)


class HostAndEntryPointTests(unittest.TestCase):
    def test_an_undeclared_host_is_refused(self):
        with mock.patch.object(snapshots.sys, "platform", "win32"):
            with self.assertRaises(snapshots.Refusal) as raised:
                snapshots.host_rid()
        self.assertIn("no declared snapshot SDK archive", str(raised.exception))

    def test_a_known_host_resolves_to_its_runtime_identifier(self):
        with mock.patch.object(snapshots.sys, "platform", "linux"):
            with mock.patch.object(snapshots.platform, "machine", lambda: "x86_64"):
                self.assertEqual(snapshots.host_rid(), "linux-x64")

    def test_a_bad_root_refuses_before_any_snapshot_is_written(self):
        committed = snapshots.REPO_ROOT / snapshots.SNAPSHOTS[0].committed
        before = snapshots.sha256_file(committed)
        stderr = io.StringIO()
        with redirect_stdout(io.StringIO()), redirect_stderr(stderr):
            code = snapshots.main(["--sdk-root", os.devnull, "--check"])
        self.assertEqual(code, 2)
        self.assertIn("not a directory", stderr.getvalue())
        self.assertEqual(snapshots.sha256_file(committed), before)

    def test_the_two_owned_snapshots_are_the_ci_regenerated_ones(self):
        self.assertEqual(
            sorted(snapshot.committed for snapshot in snapshots.SNAPSHOTS),
            [
                "fixtures/csharp-compiler-facts/compiler-facts.json",
                "fixtures/csharp-context/context.json",
            ],
        )
        for snapshot in snapshots.SNAPSHOTS:
            self.assertTrue((snapshots.REPO_ROOT / snapshot.committed).is_file())


if __name__ == "__main__":
    unittest.main()
