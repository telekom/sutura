#!/usr/bin/env python3
"""Render rustdoc JSON into the markdown pages under `docs/api/`.

WHY THIS EXISTS. mkdocs indexes markdown, not HTML. rustdoc emits its own static site with its
own search index, and the two indexes cannot be merged - so publishing rustdoc HTML gives a
second search box rather than one. mkdocstrings solves the same problem for Python by reading
STRUCTURED data and emitting pages; rustdoc JSON is the structured data for Rust, so the same
shape works here. The pages this writes are indexed by the site's own search.

WHY IT IS PYTHON. It is maintenance-time tooling that runs when somebody regenerates the pages,
never on a gate and never on the query path - the same standing as `.agents/skills-sync.py`. It
uses the standard library only, so the pixi interpreter is all it needs.

WHY THE PAGES ARE COMMITTED. rustdoc JSON is nightly-only. This repository pins a nightly for
the cranelift backend, but CI has stable and nothing else, so a docs job cannot produce the
JSON. Committing the output is the same arrangement as the ms-rust skill files: one owner, a
header saying so, and never hand-edited.

    WHAT HOLDS THAT: `cargo xtask check-api-docs` regenerates every page into a temporary
    directory and byte-compares, so a page that falls behind its sources fails rather than
    reads as current. It is a nix check, and `just api` is the fix it names.

USAGE, from the repository root, inside the dev shell so `cargo` is the pinned nightly:

    cargo rustdoc -q -p sutura-domain --all-features -- -Z unstable-options --output-format json
    pixi run --frozen python docs/.tools/rustdoc_to_markdown.py target/doc/sutura_domain.json

The output directory defaults to `docs/api`. Every page it writes must be in the `nav` in
`mkdocs.yml`, because `cargo xtask check-docs` fails on a page no nav entry names - which is the
check that catches a new crate whose page nobody added.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys
from urllib.parse import urlsplit

# rustdoc JSON is explicitly unstable and this field moves between nightlies. Reading it and
# refusing to guess is the difference between a tool and a trap: an unchecked generator either
# crashes on a field rename or, worse, silently emits a page with the interesting parts missing.
#
# Bump this together with `devco/rust-toolchain-nightly.toml`, in the same commit, and regenerate the
# pages so the diff shows what the new format changed.
EXPECTED_FORMAT_VERSION = 61

HEADER = (
    "<!-- GENERATED FILE - do not edit.\n"
    "     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.\n"
    "     Edit the doc comments in the crate source instead, then regenerate.\n"
    "     The commands are on the API reference landing page. -->\n"
)

# Auto traits and blanket impls rustdoc synthesises for every type. Listing them says nothing
# about this crate and would bury the two or three impls that do.
NOISE_TRAITS = frozenset(
    {
        "Any",
        "Borrow",
        "BorrowMut",
        "CloneToUninit",
        "DeserializeOwned",
        "From",
        "Into",
        "ToOwned",
        "TryFrom",
        "TryInto",
    }
)


class Unsupported(Exception):
    """A shape this renderer does not know how to print.

    Raised rather than papered over. A page that renders `<unknown>` where a type should be is
    worse than a failed run, because nobody reads a published page as a bug report.
    """


class Doc:
    """One crate's rustdoc JSON, with the lookups the renderer needs."""

    def __init__(self, data: dict) -> None:
        self.index = data["index"]
        self.root = str(data["root"])

    def item(self, ident) -> dict | None:
        return self.index.get(str(ident))

    def is_public(self, item: dict) -> bool:
        # `default` is what an associated item in a public impl carries; the impl's own
        # visibility is what decides there, so an impl's children are always reachable.
        return item.get("visibility") in ("public", "default")


# ---------------------------------------------------------------------------- types ---


def render_args(args) -> str:
    if not args:
        return ""
    kind, val = next(iter(args.items()))
    if kind == "angle_bracketed":
        parts = []
        for arg in val.get("args") or []:
            akind, aval = next(iter(arg.items()))
            if akind == "type":
                parts.append(render_type(aval))
            elif akind == "lifetime":
                parts.append(aval)
            elif akind == "const":
                parts.append(str(aval.get("expr", "_")))
            else:
                raise Unsupported(f"generic arg `{akind}`")
        for constraint in val.get("constraints") or []:
            parts.append(str(constraint.get("name", "_")))
        return "<" + ", ".join(parts) + ">" if parts else ""
    if kind == "parenthesized":
        inputs = ", ".join(render_type(t) for t in val.get("inputs") or [])
        output = val.get("output")
        tail = f" -> {render_type(output)}" if output else ""
        return f"({inputs}){tail}"
    raise Unsupported(f"generic args `{kind}`")


def render_bound(bound) -> str:
    kind, val = next(iter(bound.items()))
    if kind == "trait_bound":
        trait = val["trait"]
        prefix = "?" if val.get("modifier") == "maybe" else ""
        return prefix + trait["path"] + render_args(trait.get("args"))
    if kind == "outlives":
        return val
    if kind == "use":
        return "use<..>"
    raise Unsupported(f"bound `{kind}`")


def render_type(ty) -> str:
    if isinstance(ty, str):
        # `infer` and the like arrive as a bare string tag.
        return "_" if ty == "infer" else ty
    kind, val = next(iter(ty.items()))
    if kind == "resolved_path":
        return val["path"] + render_args(val.get("args"))
    if kind == "generic":
        return val
    if kind == "primitive":
        return val
    if kind == "borrowed_ref":
        lifetime = val.get("lifetime")
        mutable = "mut " if val.get("is_mutable") else ""
        head = "&" + (f"{lifetime} " if lifetime else "") + mutable
        return head + render_type(val["type"])
    if kind == "raw_pointer":
        return ("*mut " if val.get("is_mutable") else "*const ") + render_type(val["type"])
    if kind == "tuple":
        return "(" + ", ".join(render_type(t) for t in val) + ")"
    if kind == "slice":
        return "[" + render_type(val) + "]"
    if kind == "array":
        return "[" + render_type(val["type"]) + "; " + str(val.get("len", "_")) + "]"
    if kind == "impl_trait":
        return "impl " + " + ".join(render_bound(b) for b in val)
    if kind == "dyn_trait":
        traits = " + ".join(
            t["trait"]["path"] + render_args(t["trait"].get("args")) for t in val.get("traits") or []
        )
        lifetime = val.get("lifetime")
        return "dyn " + traits + (f" + {lifetime}" if lifetime else "")
    if kind == "qualified_path":
        self_type = render_type(val["self_type"])
        trait = val.get("trait")
        name = val["name"]
        if trait:
            return f"<{self_type} as {trait['path']}>::{name}"
        return f"{self_type}::{name}"
    if kind == "function_pointer":
        sig = val["signature"]
        inputs = ", ".join(render_type(t) for _, t in sig.get("inputs") or [])
        output = sig.get("output")
        tail = f" -> {render_type(output)}" if output else ""
        return f"fn({inputs}){tail}"
    raise Unsupported(f"type `{kind}`")


def render_generics(generics) -> str:
    params = []
    for param in (generics or {}).get("params") or []:
        # `fn f(x: impl Into<String>)` desugars to a type parameter whose NAME is the literal
        # text `impl Into<String>`. Printing it puts the bound in the signature twice, as
        # `fn f<impl Into<String>: Into<String>>(x: impl Into<String>)`, which is not Rust
        # anybody wrote. The argument position already carries it, so the parameter is dropped.
        if param.get("name", "").startswith("impl "):
            continue
        kind = param.get("kind") or {}
        which = next(iter(kind)) if isinstance(kind, dict) else str(kind)
        if which == "lifetime":
            params.append(param["name"])
        elif which == "type":
            bounds = kind["type"].get("bounds") or []
            rendered = " + ".join(render_bound(b) for b in bounds)
            params.append(param["name"] + (f": {rendered}" if rendered else ""))
        elif which == "const":
            params.append(f"const {param['name']}: {render_type(kind['const']['type'])}")
        else:
            raise Unsupported(f"generic param `{which}`")
    return "<" + ", ".join(params) + ">" if params else ""


def render_function(name: str, inner: dict) -> str:
    sig = inner["sig"]
    header = inner.get("header") or {}
    words = ""
    if header.get("is_const"):
        words += "const "
    if header.get("is_async"):
        words += "async "
    if header.get("is_unsafe"):
        words += "unsafe "
    inputs = []
    for arg_name, arg_type in sig.get("inputs") or []:
        rendered = render_type(arg_type)
        # `self` arrives as a normal input whose type is Self or a reference to it.
        if arg_name == "self":
            inputs.append(rendered.replace("Self", "self") if rendered.endswith("Self") else "self")
        else:
            inputs.append(f"{arg_name}: {rendered}")
    output = sig.get("output")
    tail = f" -> {render_type(output)}" if output else ""
    generics = render_generics(inner.get("generics"))
    return f"pub {words}fn {name}{generics}({', '.join(inputs)}){tail}"


# ----------------------------------------------------------------------------- docs ---

# `[`Type::method`]` is a rustdoc intra-doc link. In markdown it is a shortcut reference with no
# definition, so it renders as literal brackets around code. Dropping the brackets keeps the
# code span and loses nothing: the target is on the same page or one click away in the nav.
INTRA_DOC_LINK = re.compile(r"\[(`[^`\n]+`)\](?!\()")

# The schemes a real link may carry. **Mirrors `REAL_SCHEMES` in `xtask/src/api_links.rs`**, which
# is the gate over this file's output - and `cargo xtask check-api-links` fails if the two lists
# disagree, because a scheme this dropped and the gate allowed would silently delete a working
# link from a page.
REAL_SCHEMES = ("http", "https")

# One inline link. The destination group tolerates ONE level of nested parentheses so the call
# form `](crate::plan::run())` is matched whole - matching to the first `)` rewrote the text and
# left the second `)` behind.
INLINE_LINK = re.compile(r"\[([^\]\n]+)\]\(((?:[^()\n]|\([^()\n]*\))*)\)")


def link_destination(written: str) -> str:
    """The destination as mkdocs reads it: no angle brackets, no title.

    Both are ordinary CommonMark around the same destination, so stripping them here is what
    keeps `](<crate::x>)` and `](crate::x 'why')` from being two shapes this cannot rewrite -
    and both were, until review measured them.
    """
    first = written.split()[0] if written.split() else ""
    if first.startswith("<") and first.endswith(">"):
        first = first[1:-1]
    return first


def unfollowable(written: str) -> bool:
    """Would mkdocs fail to follow this destination?

    IT ASKS MKDOCS' OWN QUESTION, with mkdocs' own function - `urlsplit` from the standard
    library, not a Rust grammar. That is the whole design, and the reason is measured: a version
    of the gate over this file's output validated every segment of the destination as a Rust
    identifier, and five shapes with a non-identifier tail (`()`, `#anchor`, `?query`, `/path`, a
    non-ASCII segment) published a dead href at exit 0 with every mechanism green.

    Two answers matter, and they are the two `urlsplit` gives:

      * A SCHEME mkdocs does not recognise. `crate::path::Foo` parses as a URL whose scheme is
        `crate`, so mkdocs leaves it alone and publishes it verbatim - a dead href, silently, at
        exit 0. `github.com/telekom/sutura#321` counted 78 of them.
      * NO scheme. `sutura_domain::plan::Foo` has none, because `_` is not a scheme character, so
        mkdocs resolves it against the pages on disk and `--strict` ABORTS - measured on #352,
        where one such line failed the site build with every local gate green. Every crate here is
        `sutura-x`, i.e. `sutura_x` as a path, so the two are one defect and the silent one is a
        rename away from the loud one.

    rustdoc keeps the link for a reader of `cargo doc`; the page keeps the text.

    THE LIMIT: with no scheme, only a `::` spelling is dropped. A single-segment `](Foo)` is
    indistinguishable from a relative link to a page, so it is left alone and `mkdocs --strict`
    aborts on it - `just docs` inside `just validate` is the venue that says so.
    """
    dest = link_destination(written)
    try:
        scheme = urlsplit(dest).scheme
    except ValueError:
        # urlsplit refuses some bracketed hosts. mkdocs runs the same function, so leaving the
        # link alone hands the decision to the site build rather than guessing here.
        return False
    if scheme:
        return scheme not in REAL_SCHEMES
    return "::" in dest


def drop_unfollowable(match: re.Match[str]) -> str:
    return match.group(1) if unfollowable(match.group(2)) else match.group(0)


def clean_docs(text: str | None) -> str:
    if not text:
        return ""
    text = INLINE_LINK.sub(drop_unfollowable, text)
    return INTRA_DOC_LINK.sub(r"\1", text).strip()


def paragraphs(text: str) -> list[str]:
    return [p for p in (clean_docs(text) or "").split("\n\n") if p.strip()]


# ---------------------------------------------------------------------------- pages ---


class Page:
    def __init__(self) -> None:
        self.lines: list[str] = []

    def add(self, text: str = "") -> None:
        self.lines.append(text)

    def heading(self, level: int, text: str) -> None:
        self.add()
        self.add("#" * level + " " + text)
        self.add()

    def docs(self, text: str | None) -> None:
        body = clean_docs(text)
        if body:
            self.add(body)
            self.add()

    def code(self, text: str) -> None:
        self.add("```rust")
        self.add(text)
        self.add("```")
        self.add()

    def render(self) -> str:
        out = "\n".join(self.lines)
        out = re.sub(r"\n{3,}", "\n\n", out).strip()
        return out + "\n"


def item_kind(item: dict) -> str:
    inner = item["inner"]
    return next(iter(inner)) if isinstance(inner, dict) else str(inner)


def struct_or_enum_body(doc: Doc, page: Page, item: dict, level: int) -> None:
    inner = item["inner"][item_kind(item)]

    if item_kind(item) == "enum":
        variants = [doc.item(v) for v in inner.get("variants") or []]
        variants = [v for v in variants if v]
        if variants:
            page.heading(level, "Variants")
            for variant in variants:
                page.add(f"- `{variant['name']}`" + (f" - {one_line(variant.get('docs'))}" if variant.get("docs") else ""))
            page.add()

    methods: list[dict] = []
    traits: list[str] = []
    for impl_id in inner.get("impls") or []:
        impl_item = doc.item(impl_id)
        if impl_item is None:
            continue
        impl = impl_item["inner"]["impl"]
        if impl.get("is_synthetic") or impl.get("blanket_impl"):
            continue
        trait = impl.get("trait")
        if trait is None:
            for member in impl.get("items") or []:
                child = doc.item(member)
                if child and item_kind(child) == "function" and doc.is_public(child):
                    methods.append(child)
            continue
        name = trait["path"]
        if name not in NOISE_TRAITS and not name.startswith("Structural"):
            traits.append(name + render_args(trait.get("args")))

    if methods:
        # Sorted, because `cargo xtask check-api-docs` byte-compares a fresh generation against
        # the committed pages. `methods` was collected in the order of `Struct.impls`, which is
        # a rustdoc-internal list, and then in item order within each impl - stable in practice
        # on a given nightly and given source, but nothing guarantees it across either. A
        # non-deterministic generator turns that gate into a coin flip, which is worse than no
        # gate. `traits` below was already sorted for the same reason.
        methods.sort(key=lambda method: method["name"] or "")
        page.heading(level, "Methods")
        for method in methods:
            page.code(render_function(method["name"], method["inner"]["function"]))
            page.docs(method.get("docs"))

    if traits:
        page.heading(level, "Implements")
        page.add(", ".join(f"`{t}`" for t in sorted(set(traits))))
        page.add()


def one_line(text: str | None) -> str:
    body = clean_docs(text)
    return body.split("\n\n")[0].replace("\n", " ") if body else ""


def render_module(doc: Doc, page: Page, module_id: str, level: int) -> None:
    module = doc.item(module_id)
    if module is None:
        return
    inner = module["inner"]["module"]

    children = [doc.item(i) for i in inner.get("items") or []]
    children = [c for c in children if c and doc.is_public(c)]

    submodules = [c for c in children if item_kind(c) == "module"]
    types = [c for c in children if item_kind(c) in ("struct", "enum", "union", "trait")]
    functions = [c for c in children if item_kind(c) == "function"]
    others = [c for c in children if c not in submodules + types + functions]

    for item in types:
        kind = item_kind(item)
        page.heading(level, f"`{kind} {item['name']}`")
        generics = render_generics(item["inner"][kind].get("generics"))
        page.code(f"pub {kind} {item['name']}{generics}")
        page.docs(item.get("docs"))
        struct_or_enum_body(doc, page, item, level + 1)

    for item in functions:
        page.heading(level, f"`fn {item['name']}`")
        page.code(render_function(item["name"], item["inner"]["function"]))
        page.docs(item.get("docs"))

    for item in others:
        page.heading(level, f"`{item_kind(item)} {item['name']}`")
        page.docs(item.get("docs"))

    for sub in submodules:
        page.heading(level, f"Module `{sub['name']}`")
        page.docs(sub.get("docs"))
        render_module(doc, page, str(sub["id"]), level + 1)


def render_crate(data: dict, crate_name: str) -> str:
    found = data.get("format_version")
    if found != EXPECTED_FORMAT_VERSION:
        raise SystemExit(
            "rustdoc JSON format_version mismatch: found "
            f"{found}, expected {EXPECTED_FORMAT_VERSION}.\n"
            "rustdoc JSON is unstable and this number changes between nightlies. Read the new\n"
            "format, update EXPECTED_FORMAT_VERSION in docs/.tools/rustdoc_to_markdown.py in\n"
            "the same commit as devco/rust-toolchain-nightly.toml, and regenerate the pages so the\n"
            "diff shows what changed."
        )

    doc = Doc(data)
    root = doc.item(doc.root)
    if root is None:
        raise SystemExit("rustdoc JSON has no root item")

    page = Page()
    page.add(HEADER.rstrip("\n"))
    page.add()
    page.add(f"# {crate_name}")
    page.add()
    # NO VERSION in this line, deliberately. It used to print `crate_version`, which made the
    # committed page a second copy of the number in Cargo.toml - and `check-api-docs` byte-
    # compares, so a release bump alone turned the page stale and failed the NEXT pull request
    # to run the gate. That happened: v0.2.0 shipped and the following PR went red on a page
    # whose doc comments nobody had touched.
    #
    # Keeping the two in sync was the alternative, and it needs the nightly toolchain and this
    # generator wired into the release path - machinery to maintain a duplicate. Dropping the
    # duplicate is cheaper and cannot drift. The site carries the version already: mike
    # publishes one docs tree per release and its selector names them.
    page.add(f"The public API of `{crate_name}`, rendered from rustdoc JSON.")
    page.add()
    page.docs(root.get("docs"))
    render_module(doc, page, doc.root, 2)
    return page.render()


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__)
        return 2
    out_dir = pathlib.Path(argv[-1]) if len(argv) > 2 and not argv[-1].endswith(".json") else pathlib.Path("docs/api")
    inputs = [pathlib.Path(a) for a in argv[1:] if a.endswith(".json")]
    if not inputs:
        raise SystemExit("no rustdoc JSON file given")
    out_dir.mkdir(parents=True, exist_ok=True)

    for path in inputs:
        data = json.loads(path.read_text(encoding="utf-8"))
        # `sutura_domain.json` documents the crate `sutura-domain`. rustdoc names the file after
        # the crate's Rust identifier; the page is named after the package, which is what a
        # reader looks for in the nav and in Cargo.toml.
        crate_name = path.stem.replace("_", "-")
        text = render_crate(data, crate_name)
        target = out_dir / f"{crate_name}.md"
        target.write_bytes(text.encode("utf-8"))
        print(f"wrote {target} ({len(text.splitlines())} lines)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
