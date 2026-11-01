#!/usr/bin/env python3
"""Validate Veldir's cooperative custom-agent roster without extra dependencies.

This checks the deliberately small YAML frontmatter subset we author, not
the full YAML specification. Runtime interoperability must also be tested
inside the actual GitHub Copilot/VS Code harness.
"""
import ast
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AGENTS_DIR = ROOT / ".github" / "agents"
EXPECTED = {
    "veldir-orchestrator",
    "veldir-rust-core",
    "veldir-transfer-safety",
    "veldir-windows",
    "veldir-ux",
    "veldir-search-preview",
    "veldir-qa",
    "veldir-performance",
    "veldir-security",
    "veldir-release",
}
SHARED = [
    "AGENTS.md",
    "docs/AGENT_TEAM.md",
    ".github/copilot-instructions.md",
    ".github/ISSUE_TEMPLATE/agent-task.md",
]
REQUIRED_TOOLS = {"read", "search", "agent"}
KNOWN_TOOLS = REQUIRED_TOOLS | {"edit", "execute"}

errors: list[str] = []


def fail(message: str) -> None:
    errors.append(message)


def parse_frontmatter(path: Path) -> tuple[dict[str, str], str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0] != "---":
        fail(f"{path}: missing opening YAML separator")
        return {}, ""
    try:
        closing = lines.index("---", 1)
    except ValueError:
        fail(f"{path}: missing closing YAML separator")
        return {}, ""
    fields: dict[str, str] = {}
    for line in lines[1:closing]:
        if line.startswith((" ", "\t")):
            continue  # handoff subfields, validated below
        if ":" not in line:
            fail(f"{path}: invalid header line {line!r}")
            continue
        key, value = line.split(":", 1)
        if key in fields:
            fail(f"{path}: duplicate frontmatter field {key}")
        fields[key] = value.strip()
    return fields, "\n".join(lines[closing + 1 :])


def yaml_inline(value: str, path: Path, field: str):
    try:
        return ast.literal_eval(value)
    except (ValueError, SyntaxError) as exc:
        fail(f"{path}: {field} must be a quoted scalar or list: {exc}")
        return None


for missing in SHARED:
    if not (ROOT / missing).is_file():
        fail(f"Required shared file is missing: {missing}")

paths = list(AGENTS_DIR.glob("*.agent.md"))
found = {path.name.removesuffix(".agent.md") for path in paths}
if found != EXPECTED:
    fail(f"Roster mismatch: missing={sorted(EXPECTED - found)} extra={sorted(found - EXPECTED)}")

handoff_count = 0
for path in sorted(paths):
    fm, body = parse_frontmatter(path)
    slug = path.name.removesuffix(".agent.md")
    if fm.get("name") != slug:
        fail(f"{path}: name must exactly match file slug {slug}")
    if yaml_inline(fm.get("description", ""), path, "description") in (None, ""):
        fail(f"{path}: nonempty description required")
    tools = yaml_inline(fm.get("tools", ""), path, "tools")
    if not isinstance(tools, list) or not REQUIRED_TOOLS.issubset(set(tools)):
        fail(f"{path}: missing required read/search/agent tools")
    elif not set(tools).issubset(KNOWN_TOOLS):
        fail(f"{path}: unknown tool aliases {sorted(set(tools) - KNOWN_TOOLS)}")
    if slug == "veldir-security" and isinstance(tools, list):
        if {"edit", "execute"} & set(tools):
            fail(f"{path}: security reviewer must be read-only")
    if yaml_inline(fm.get("agents", ""), path, "agents") != ["*"]:
        fail(f"{path}: all peers must be available as subagents")
    if fm.get("disable-model-invocation") != "false":
        fail(f"{path}: subagent invocation must be enabled")
    if fm.get("user-invocable") != "true":
        fail(f"{path}: user invocation must be enabled")
    if fm.get("handoffs") != "":
        fail(f"{path}: required handoffs section absent")
    headers = path.read_text(encoding="utf-8").split("---", 2)
    yaml_part = headers[1] if len(headers) == 3 else ""
    peers = re.findall(r"^\s+agent:\s*([a-z0-9-]+)\s*$", yaml_part, re.MULTILINE)
    handoff_count += len(peers)
    if len(peers) < 2:
        fail(f"{path}: expected at least two specific handoffs")
    for peer in peers:
        if peer not in EXPECTED:
            fail(f"{path}: handoff to unknown agent {peer}")
        if peer == slug:
            fail(f"{path}: self-handoff would risk an unbounded loop")
    if "VELDIR-ASK" not in body or "VELDIR-ANSWER" not in body:
        fail(f"{path}: missing shared two-way communication contract")
    if "AGENTS.md" not in body or "AGENT_TEAM.md" not in body:
        fail(f"{path}: shared instructions not referenced")
    if len(body) > 30_000:
        fail(f"{path}: prompt too long")

try:
    settings = json.loads((ROOT / ".vscode" / "settings.json").read_text(encoding="utf-8"))
    if settings.get("chat.subagents.allowInvocationsFromSubagents") is not True:
        fail("VS Code nested subagent consultation is not enabled")
except (OSError, ValueError) as exc:
    fail(f"Invalid .vscode/settings.json: {exc}")

if errors:
    print("Agent team configuration FAILED:")
    for error in errors:
        print(" -", error)
    sys.exit(1)

print(f"Agent team OK: {len(paths)} roles, {handoff_count} validated handoff targets.")
print("This validates configuration only, not a live multi-agent conversation.")
