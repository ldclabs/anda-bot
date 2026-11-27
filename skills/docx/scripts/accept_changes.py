"""Accept Word text, move, paragraph-mark and formatting revisions in a new DOCX."""

import argparse
import tempfile
import zipfile
from pathlib import Path

from lxml import etree

from office.helpers import rezip, safe_extract

WORD_NAMESPACES = {
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
}
RUN_CONTAINERS = {"p", "hyperlink", "sdtContent", "customXml", "smartTag", "fldSimple",
                  "ins", "del", "moveFrom", "moveTo", "dir", "bdo"}
FORMATTING = {"rPrChange", "pPrChange", "sectPrChange", "tblPrChange", "tblPrExChange",
              "tblGridChange", "trPrChange", "tcPrChange", "numberingChange"}


MOVE_RANGES = {"moveFromRangeStart", "moveFromRangeEnd", "moveToRangeStart", "moveToRangeEnd"}


def accept_part(tree):
    changed = False
    joins = []
    # Only visit the accepted view. A deleted run can contain older nested revisions.
    pending = list(reversed(tree.getroot()))
    while pending:
        node = pending.pop()
        if not isinstance(node.tag, str):
            continue
        parent = node.getparent()
        tag = etree.QName(node)
        kind = tag.localname
        if tag.namespace not in WORD_NAMESPACES:
            if kind in {"conflictIns", "conflictDel"}:
                raise ValueError(f"Resolve the {kind} conflict in a Word editor")
            pending.extend(reversed(node))
            continue

        word = "{" + tag.namespace + "}"
        if kind in FORMATTING or kind in MOVE_RANGES:
            # The current properties are on the parent; the change holds the previous ones.
            parent.remove(node)
        elif kind in {"ins", "del", "moveFrom", "moveTo"}:
            keep = kind in {"ins", "moveTo"}
            if parent.tag == word + "rPr" and parent.getparent().tag == word + "pPr":
                if not keep:
                    joins.append(parent.getparent().getparent())
            elif parent.tag == word + "trPr" and keep:
                pass  # Keep an inserted table row and remove its revision marker.
            elif etree.QName(parent).localname in RUN_CONTAINERS:
                if keep:
                    children = list(node)
                    for child in children:
                        node.addprevious(child)
                    pending.extend(reversed(children))
            else:
                raise ValueError(f"Unsupported {kind} revision inside {etree.QName(parent).localname}")
            parent.remove(node)
        elif kind in {"cellIns", "cellDel", "cellMerge"} or kind.startswith(("customXmlInsRange", "customXmlDelRange", "customXmlMove")):
            raise ValueError(f"Resolve the {kind} structural revision in a Word editor")
        else:
            pending.extend(reversed(node))
            continue
        changed = True

    # Each surviving paragraph mark owns the resulting paragraph's formatting.
    for paragraph in joins:
        following = paragraph.getnext()
        if following is None or following.tag != paragraph.tag:
            raise ValueError("Cannot join a deleted paragraph mark across a structural boundary")
        properties_tag = etree.QName(etree.QName(paragraph).namespace, "pPr").text
        offset = int(len(following) > 0 and following[0].tag == properties_tag)
        for child in reversed(list(paragraph)):
            if child.tag != properties_tag:
                following.insert(offset, child)
        paragraph.getparent().remove(paragraph)
    return changed


def accept_changes(input_file: str, output_file: str) -> tuple[None, str]:
    source = Path(input_file).resolve()
    output = Path(output_file).resolve()
    if source == output:
        return None, "Error: choose a separate output file"
    try:
        with tempfile.TemporaryDirectory() as temp:
            folder = Path(temp)
            with zipfile.ZipFile(source) as archive:
                safe_extract(archive, folder)
            if not (folder / "word/document.xml").is_file():
                raise ValueError("Missing word/document.xml")
            parser = etree.XMLParser(resolve_entities=False, no_network=True)
            for part in (folder / "word").rglob("*.xml"):
                tree = etree.parse(str(part), parser)
                if accept_part(tree):
                    tree.write(str(part), xml_declaration=True, encoding="UTF-8", standalone=True)
            output.parent.mkdir(parents=True, exist_ok=True)
            rezip(folder, output)
    except (OSError, ValueError, zipfile.BadZipFile, etree.XMLSyntaxError) as error:
        return None, f"Error: {error}"
    return None, f"Accepted tracked changes in {output}"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_file")
    parser.add_argument("output_file")
    args = parser.parse_args()
    _, message = accept_changes(args.input_file, args.output_file)
    print(message)
    raise SystemExit(1 if message.startswith("Error:") else 0)
