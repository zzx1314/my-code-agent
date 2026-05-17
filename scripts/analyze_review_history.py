#!/usr/bin/env python3
"""
Analyze Review History from Saved Sessions
===========================================

Scans session files (.session.json, .sessions/*.json) for embedded code review
reports, classifies issues as potential false positives vs real issues, and
compares rates before and after the review agent fix (2026-05-17).

Usage:
    python scripts/analyze_review_history.py
    python scripts/analyze_review_history.py --session-dir .sessions
    python scripts/analyze_review_history.py --verbose
    python scripts/analyze_review_history.py --export-csv report.csv

Output:
    - Statistics summary printed to stdout
    - Optionally export all issues as CSV
    - Colorful terminal output with emoji indicators

How it works:
    1. Scans session directories for .json files
    2. Parses chat_history for messages containing review reports
    3. Extracts JSON issues from LLM responses (matches the same
       patterns as extract_json_from_response in review_agent.rs)
    4. Classifies issues:
       - "likely_real": Security, ErrorHandling, Performance with
         concrete file/line references, or issues with specific
         suggestions/fix_examples
       - "likely_false_positive": FunctionalCompleteness or BugRisk
         issues about "registration", "mapping", "wiring", "missing"
         wiring patterns that match the known false positive scenario
       - "uncertain": everything else
    5. Groups by date and compares before/after the fix date
    6. Calculates false positive rates

False Positive Heuristics
-------------------------
The classifier flags issues as potential false positives based on:

    CATEGORY TRIGGERS:
    - FunctionalCompleteness: issues about "registration", "wiring",
      "mapping", "dispatch", "import" that are outside the diff
    - BugRisk: issues about "undefined", "potentially null", "might
      be undefined" that reference code outside the diff

    PATTERN TRIGGERS:
    - Title contains "registration", "mapping", "wiring", "dispatch",
      "NotFound", "not found", "undefined"
    - Description mentions "from_extension", "parser dispatch",
      "isn't in the diff", "not present in the diff"
    - Issue category is FunctionalCompleteness and the description
      mentions verifying tests passed

    These heuristics match the specific false positive patterns that
    the review agent fix (Phase 1 file context + global guidance)
    was designed to address.
"""

import argparse
import json
import os
import re
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional

# ─── Configuration ───────────────────────────────────────────────────────────

# The fix was committed on 2026-05-17. Sessions before this date used the OLD
# review prompt (missing file context in Phase 1, no global guidance).
FIX_DATE = datetime(2026, 5, 17, tzinfo=timezone.utc)

# Session directory defaults
DEFAULT_SESSION_DIRS = [".", ".sessions"]

# ─── False Positive Classification ───────────────────────────────────────────

# Keywords that suggest a FunctionalCompleteness issue is about missing
# wiring/registration outside the diff range (the known false positive pattern)
FP_WIRING_KEYWORDS = [
    "registration", "mapping", "wiring", "dispatch", "register",
    "from_extension", "parser dispatch", "NotFound", "not found",
    "not registered", "not mapped", "not wired",
]

# Keywords that suggest a BugRisk issue is about code outside the diff
FP_BUGRISK_KEYWORDS = [
    "undefined", "potentially null", "might be undefined",
    "may be undefined", "could be null", "not in the diff",
    "outside the diff", "not defined in",
]


def classify_issue(issue: dict) -> str:
    """Classify an issue as 'likely_false_positive', 'likely_real', or 'uncertain'.

    Uses the same patterns that the review agent fix was designed to address.
    """
    category = (issue.get("category") or "").lower()
    title = (issue.get("title") or "").lower()
    description = (issue.get("description") or "").lower()
    severity = (issue.get("severity") or "").lower()
    suggestion = issue.get("suggestion") or ""
    fix_example = issue.get("fix_example") or ""
    has_line = issue.get("line") is not None

    # ── False Positive Signals ───────────────────────────────────────────

    # FunctionalCompleteness issue about wiring/registration/mapping
    # This is THE classic false positive pattern the fix targets
    if category == "functional_completeness":
        for kw in FP_WIRING_KEYWORDS:
            if kw in title or kw in description:
                return "likely_false_positive"

    # BugRisk issue about code outside the diff range
    if category == "bug_risk":
        for kw in FP_BUGRISK_KEYWORDS:
            if kw in title or kw in description:
                return "likely_false_positive"

    # Issue that explicitly says "not in diff" or similar
    if "not in the diff" in description or "outside the diff" in description:
        return "likely_false_positive"

    # ── Real Issue Signals ────────────────────────────────────────────────

    # Security issues are always real (the system doesn't have
    # false positive patterns for security)
    if category == "security":
        return "likely_real"

    # Critical or High severity issues with concrete line numbers
    # and specific suggestions are likely real
    if severity in ("critical", "high") and has_line and suggestion:
        return "likely_real"

    # Issues with concrete fix_examples and line numbers
    if fix_example and has_line:
        return "likely_real"

    # ErrorHandling issues with line references
    if category == "error_handling" and has_line:
        return "likely_real"

    # Performance issues with line references
    if category == "performance" and has_line:
        return "likely_real"

    # ── Fallback ──────────────────────────────────────────────────────────

    return "uncertain"


# ─── Session File Parsing ────────────────────────────────────────────────────


def find_session_files(dirs: list[str]) -> list[Path]:
    """Find all session JSON files in the given directories."""
    found = []
    for d in dirs:
        p = Path(d)
        if not p.exists():
            continue
        if p.is_file() and p.suffix == ".json":
            found.append(p)
        elif p.is_dir():
            for f in sorted(p.iterdir()):
                if f.suffix == ".json":
                    found.append(f)
    return found


def parse_timestamp_from_filename(path: Path) -> Optional[datetime]:
    """Try to extract a timestamp from the session filename.

    Handles formats like:
        session_2025_01_15_10_30_00.json
        session_2026-05-17.json
        2026-05-17T14-30-00.json
    """
    stem = path.stem
    patterns = [
        r"(\d{4})[_-](\d{2})[_-](\d{2})[_-](\d{2})[_-](\d{2})[_-](\d{2})",
        r"(\d{4})[_-](\d{2})[_-](\d{2})",
    ]
    for pat in patterns:
        m = re.search(pat, stem)
        if m:
            parts = [int(x) for x in m.groups()]
            if len(parts) == 6:
                return datetime(*parts, tzinfo=timezone.utc)
            elif len(parts) == 3:
                return datetime(*parts, tzinfo=timezone.utc)
    return None


def load_session(path: Path) -> Optional[dict]:
    """Load and validate a session JSON file."""
    try:
        data = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        if "chat_history" in data and isinstance(data["chat_history"], list):
            return data
        return None
    except (json.JSONDecodeError, OSError):
        return None


# ─── Review Report Extraction ────────────────────────────────────────────────


def extract_review_reports(session_data: dict) -> list[dict]:
    """Extract review reports from session chat history.

    Looks for LLM responses that contain JSON with "issues" + "summary"
    keys — these are the output of the review agent.

    Also looks for user messages containing "Code Review - Iteration"
    to track the iteration loop and determine which issues were accepted.
    """
    reports = []
    history = session_data.get("chat_history", [])

    for i, msg in enumerate(history):
        content = msg.get("content", "")

        # Skip empty messages
        if not content:
            continue

        # Try to find JSON review reports in the message
        issues_data = extract_json_with_issues(content)
        if issues_data is not None:
            report = {
                "message_index": i,
                "role": msg.get("role", "unknown"),
                "issues": issues_data.get("issues", []),
                "summary": issues_data.get("summary", {}),
                "raw_preview": content[:200],
            }
            reports.append(report)

    return reports


def extract_json_with_issues(text: str) -> Optional[dict]:
    """Extract a JSON object containing 'issues' + 'summary' from text.

    Mirrors the strategy in review_agent.rs's extract_json_from_response:
    1. Try ```json ... ``` code block
    2. Try ``` ... ``` code block
    3. Find outermost { ... } with brace counting
    """
    # Strategy 1: ```json ... ```
    if "```json" in text:
        start = text.find("```json") + 7
        end = text.find("```", start)
        if end > start:
            candidate = text[start:end].strip()
            return _parse_issues_json(candidate)

    # Strategy 2: ``` ... ```
    # Find the LAST ``` pair
    last = text.rfind("```")
    if last > 0:
        before = text[:last]
        open_idx = before.rfind("```")
        if open_idx >= 0:
            candidate = before[open_idx + 3:].strip()
            if candidate.startswith("{") or candidate.startswith("["):
                return _parse_issues_json(candidate)

    # Strategy 3: find outermost { ... }
    brace_start = text.find("{")
    if brace_start >= 0:
        depth = 0
        json_start = None
        in_string = False
        prev_escape = False
        for idx, ch in enumerate(text[brace_start:], brace_start):
            if ch == '"' and not prev_escape:
                in_string = not in_string
            prev_escape = (ch == "\\" and not prev_escape)
            if in_string:
                continue
            if ch == "{":
                if depth == 0:
                    json_start = idx
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0 and json_start is not None:
                    candidate = text[json_start:idx + 1]
                    return _parse_issues_json(candidate)

    return None


def _parse_issues_json(text: str) -> Optional[dict]:
    """Try to parse text as JSON containing 'issues' array + 'summary'."""
    try:
        data = json.loads(text)
        if isinstance(data, dict) and "issues" in data and "summary" in data:
            return data
    except json.JSONDecodeError:
        pass
    return None


def determine_session_date(session_data: dict, path: Path) -> Optional[datetime]:
    """Determine when a session was created/saved.

    Priority:
    1. saved_at field in session data
    2. Timestamp from filename
    3. File modification time
    """
    saved_at = session_data.get("saved_at")
    if saved_at is not None:
        try:
            return datetime.fromtimestamp(int(saved_at), tz=timezone.utc)
        except (ValueError, OSError):
            pass

    ts = parse_timestamp_from_filename(path)
    if ts:
        return ts

    try:
        mtime = os.path.getmtime(path)
        return datetime.fromtimestamp(mtime, tz=timezone.utc)
    except OSError:
        return None


def is_after_fix(session_date: datetime) -> bool:
    """Check if a session date is after the fix was applied."""
    return session_date >= FIX_DATE


# ─── Analysis & Statistics ───────────────────────────────────────────────────


def analyze_sessions(session_files: list[Path], verbose: bool = False) -> dict:
    """Analyze all session files and produce comparison statistics."""
    before: list[dict] = []   # issues from before the fix
    after: list[dict] = []    # issues from after the fix
    all_reports: list[dict] = []

    for sf in session_files:
        session = load_session(sf)
        if session is None:
            if verbose:
                print(f"  ⚠️  Skipping {sf.name}: invalid or missing chat_history")
            continue

        session_date = determine_session_date(session, sf)
        if session_date is None:
            if verbose:
                print(f"  ⚠️  Skipping {sf.name}: cannot determine date")
            continue

        reports = extract_review_reports(session)
        if not reports:
            if verbose:
                print(f"  ℹ️  {sf.name} ({session_date.date()}): no review reports found")
            continue

        group = before if not is_after_fix(session_date) else after

        for rpt in reports:
            rpt["session_file"] = sf.name
            rpt["session_date"] = session_date.isoformat()
            for issue in rpt.get("issues", []):
                classification = classify_issue(issue)
                issue["_classification"] = classification
                issue["_session_file"] = sf.name
                issue["_session_date"] = session_date.isoformat()
                group.append(issue)
            all_reports.append(rpt)

        if verbose:
            issue_count = sum(len(r.get("issues", [])) for r in reports)
            review_count = len(reports)
            print(f"  📄 {sf.name} ({session_date.date()}): {review_count} reviews, "
                  f"{issue_count} issues ({'BEFORE' if not is_after_fix(session_date) else 'AFTER'} fix)")

    stats = compute_statistics(before, after, all_reports)
    return stats


def compute_statistics(before: list[dict], after: list[dict],
                       all_reports: list[dict]) -> dict:
    """Compute comparison statistics."""
    def classify_issues(issues: list[dict]) -> dict:
        if not issues:
            return {
                "total": 0,
                "likely_false_positive": 0,
                "likely_real": 0,
                "uncertain": 0,
                "fp_rate": 0.0,
                "category_counts": {},
                "severity_counts": {},
                "top_fp_patterns": [],
                "top_real_patterns": [],
            }
        classifications = Counter(i["_classification"] for i in issues)
        categories = Counter(i.get("category", "unknown") for i in issues)
        severities = Counter(i.get("severity", "unknown") for i in issues)
        total = len(issues)
        fp_count = classifications.get("likely_false_positive", 0)
        real_count = classifications.get("likely_real", 0)

        # Top false positive patterns
        fp_issues = [i for i in issues if i["_classification"] == "likely_false_positive"]
        fp_patterns = Counter()
        for i in fp_issues:
            title = i.get("title", "")
            cat = i.get("category", "")
            fp_patterns[f"[{cat}] {title[:60]}"] += 1

        # Top real patterns
        real_issues = [i for i in issues if i["_classification"] == "likely_real"]
        real_patterns = Counter()
        for i in real_issues:
            title = i.get("title", "")
            cat = i.get("category", "")
            real_patterns[f"[{cat}] {title[:60]}"] += 1

        return {
            "total": total,
            "likely_false_positive": fp_count,
            "likely_real": real_count,
            "uncertain": classifications.get("uncertain", 0),
            "fp_rate": round(fp_count / total * 100, 1) if total > 0 else 0.0,
            "real_rate": round(real_count / total * 100, 1) if total > 0 else 0.0,
            "category_counts": dict(categories),
            "severity_counts": dict(severities),
            "top_fp_patterns": fp_patterns.most_common(10),
            "top_real_patterns": real_patterns.most_common(10),
        }

    before_stats = classify_issues(before)
    after_stats = classify_issues(after)

    improvement = None
    if before_stats["total"] > 0 and after_stats["total"] > 0:
        improvement = round(
            before_stats["fp_rate"] - after_stats["fp_rate"], 1
        )

    return {
        "before": before_stats,
        "after": after_stats,
        "improvement": improvement,
        "total_reports": len(all_reports),
        "total_sessions": len(set(r["session_file"] for r in all_reports)),
        "all_issues": before + after,
    }


# ─── Output Formatting ───────────────────────────────────────────────────────


def format_statistics(stats: dict) -> str:
    """Format statistics as a human-readable report."""
    b = stats["before"]
    a = stats["after"]
    lines = []

    lines.append("=" * 60)
    lines.append("  📊 Review Agent False Positive Analysis")
    lines.append("=" * 60)
    lines.append("")

    # Summary header
    lines.append(f"  Total sessions analyzed:  {stats['total_sessions']}")
    lines.append(f"  Total review reports:     {stats['total_reports']}")
    lines.append(f"  Total issues found:       {b['total'] + a['total']}")
    lines.append(f"  Fix applied:              {FIX_DATE.date()}")
    lines.append("")

    # ── Before vs After Table ─────────────────────────────────────────────
    lines.append("  ┌─────────────────────┬──────────┬──────────┬──────────┐")
    lines.append("  │ Metric              │ Before   │ After    │ Δ        │")
    lines.append("  ├─────────────────────┼──────────┼──────────┼──────────┤")

    def fmt_row(label, before_val, after_val, suffix=""):
        b_str = f"{before_val}{suffix}" if before_val is not None else "N/A"
        a_str = f"{after_val}{suffix}" if after_val is not None else "N/A"
        delta = ""
        if before_val is not None and after_val is not None:
            d = round(after_val - before_val, 1)
            if d > 0:
                delta = f"+{d}{suffix} 📈"
            elif d < 0:
                delta = f"{d}{suffix} 📉"
            else:
                delta = f"0{suffix}"
        lines.append(f"  │ {label:<20} │ {b_str:>8} │ {a_str:>8} │ {delta:<10} │")

    fmt_row("Total issues", b["total"], a["total"])
    fmt_row("False positives", b["likely_false_positive"], a["likely_false_positive"])
    fmt_row("Real issues", b["likely_real"], a["likely_real"])
    fmt_row("Uncertain", b["uncertain"], a["uncertain"])
    fmt_row("FP rate", b["fp_rate"], a["fp_rate"], "%")
    fmt_row("Real rate", b["real_rate"], a["real_rate"], "%")

    lines.append("  └─────────────────────┴──────────┴──────────┴──────────┘")
    lines.append("")

    # Improvement callout
    if stats["improvement"] is not None:
        if stats["improvement"] > 0:
            lines.append(f"  ✅ FP rate decreased by {stats['improvement']}% after the fix! 🎉")
        elif stats["improvement"] < 0:
            lines.append(f"  ⚠️  FP rate increased by {abs(stats['improvement'])}% — investigate further")
        else:
            lines.append("  ℹ️  FP rate unchanged")
    else:
        lines.append("  ℹ️  Insufficient data to compare (need both before and after samples)")
    lines.append("")

    # ── Category Breakdown ────────────────────────────────────────────────
    if b["total"] > 0 or a["total"] > 0:
        lines.append("  ── Issue Categories ──")
        all_cats = set(list(b["category_counts"].keys()) + list(a["category_counts"].keys()))
        for cat in sorted(all_cats):
            b_c = b["category_counts"].get(cat, 0)
            a_c = a["category_counts"].get(cat, 0)
            lines.append(f"    {cat:<30}  Before: {b_c:<4}  After: {a_c:<4}")

        if b["top_fp_patterns"] or a["top_fp_patterns"]:
            lines.append("")
            lines.append("  ── Top False Positive Patterns ──")
            seen = set()
            for pattern, count in (b.get("top_fp_patterns", []) or []) + (a.get("top_fp_patterns", []) or []):
                if pattern not in seen:
                    seen.add(pattern)
                    lines.append(f"    ⚠️  {pattern:<65} ({count}x)")

        if b["top_real_patterns"] or a["top_real_patterns"]:
            lines.append("")
            lines.append("  ── Top Real Issue Patterns ──")
            seen = set()
            for pattern, count in (b.get("top_real_patterns", []) or []) + (a.get("top_real_patterns", []) or []):
                if pattern not in seen:
                    seen.add(pattern)
                    lines.append(f"    🐛 {pattern:<65} ({count}x)")

    lines.append("")
    lines.append("=" * 60)

    return "\n".join(lines)


def export_csv(stats: dict, filepath: str):
    """Export all issues to a CSV file."""
    import csv

    issues = stats["all_issues"]
    if not issues:
        print(f"  ℹ️  No issues to export")
        return

    fieldnames = [
        "session_file", "session_date", "category", "severity",
        "title", "description", "suggestion", "classification",
        "file", "line", "end_line",
    ]

    with open(filepath, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames, extrasaction="ignore")
        writer.writeheader()

        for issue in sorted(issues, key=lambda x: (x.get("_session_date", ""), x.get("category", ""))):
            row = {
                "session_file": issue.get("_session_file", ""),
                "session_date": issue.get("_session_date", ""),
                "category": issue.get("category", ""),
                "severity": issue.get("severity", ""),
                "title": issue.get("title", ""),
                "description": issue.get("description", ""),
                "suggestion": issue.get("suggestion", ""),
                "classification": issue.get("_classification", ""),
                "file": issue.get("file", ""),
                "line": issue.get("line", ""),
                "end_line": issue.get("end_line", ""),
            }
            writer.writerow(row)

    print(f"  📁 Exported {len(issues)} issues to {filepath}")


# ─── Main ────────────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(
        description="Analyze review history for false positive rates",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument(
        "--session-dir", "-s",
        action="append",
        default=[],
        help="Directory or file to scan for sessions (can specify multiple)",
    )
    parser.add_argument(
        "--verbose", "-v",
        action="store_true",
        help="Show per-file details",
    )
    parser.add_argument(
        "--export-csv", "-o",
        type=str,
        default=None,
        help="Export all issues to a CSV file",
    )
    parser.add_argument(
        "--fix-date",
        type=str,
        default=None,
        help=f"Override fix date (default: {FIX_DATE.date()})",
    )

    args = parser.parse_args()

    # Use provided dirs or defaults
    dirs = args.session_dir if args.session_dir else DEFAULT_SESSION_DIRS

    # Override fix date if specified
    global FIX_DATE
    if args.fix_date:
        try:
            FIX_DATE = datetime.fromisoformat(args.fix_date).replace(tzinfo=timezone.utc)
        except ValueError:
            print(f"  ❌ Invalid date format: {args.fix_date}. Use YYYY-MM-DD.")
            sys.exit(1)

    # Find session files
    session_files = find_session_files(dirs)
    if not session_files:
        print(f"  ❌ No session files found in: {', '.join(dirs)}")
        print(f"  ℹ️  Run the application first to generate session data, then:")
        print(f"     python scripts/analyze_review_history.py --session-dir .sessions")
        sys.exit(0)

    print(f"  📂 Found {len(session_files)} session file(s)")
    print(f"  🔍 Analyzing...")
    print()

    # Analyze
    stats = analyze_sessions(session_files, verbose=args.verbose)

    # Print report
    report = format_statistics(stats)
    print(report)

    # Export CSV
    if args.export_csv:
        export_csv(stats, args.export_csv)

    # Summary with actionable advice
    b = stats["before"]
    a = stats["after"]

    if b["total"] == 0 and a["total"] == 0:
        print()
        print("  💡 No review data found in sessions.")
        print("     Sessions will accumulate as you use the application.")
        print("     Re-run this script after saving sessions with /save or on quit.")
    elif b["total"] == 0:
        print()
        print("  💡 Only 'after fix' data found. Keep using the application")
        print("     and re-run this script as more data accumulates.")
    elif a["total"] == 0:
        print()
        print("  💡 Only 'before fix' data found. After the fix has been")
        print("     deployed, new sessions will be classified as 'after fix'.")


if __name__ == "__main__":
    main()
