"""Enforce the allowed normal dependency graph, using Cargo's own metadata."""
import json
import subprocess
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"]))
allowed = {
    "sysgud-core": set(),
    "sysgud-runtime": {"sysgud-core"},
    "sysgud-api": {"sysgud-core", "sysgud-runtime"},
    "sysgud-telegram": {"sysgud-core"},
    "sysgud": {"sysgud-core", "sysgud-runtime", "sysgud-api", "sysgud-telegram"},
}
for package in metadata["packages"]:
    normal = {d["name"] for d in package["dependencies"] if d["kind"] is None}
    internal = {name for name in normal if name.startswith("sysgud")}
    assert internal <= allowed[package["name"]], f"Invalid crate boundary: {package['name']}: {internal}"
    if package["name"] == "sysgud-core":
        assert normal <= {"serde", "chrono", "uuid"}, f"Domain gained I/O dependencies: {normal}"
print("Crate dependency boundaries: OK")
