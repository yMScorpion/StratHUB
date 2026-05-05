"""PDF text extraction via PyMuPDF, with optional Tesseract OCR fallback."""

from __future__ import annotations

import io
import logging

from .models import ExtractedPdf, PageContent

_LOG = logging.getLogger(__name__)

# Pages with fewer native chars than this threshold trigger an OCR attempt.
_NATIVE_CHAR_MIN = 100
# Pages still below this after OCR are flagged as chart-heavy.
_CHART_HEAVY_THRESHOLD = 50

try:
    import fitz  # PyMuPDF

    _HAS_PYMUPDF = True
except ImportError:
    _HAS_PYMUPDF = False
    _LOG.warning("PyMuPDF not installed; PDF extraction will be unavailable")

try:
    import pytesseract
    from PIL import Image

    pytesseract.get_tesseract_version()
    _HAS_TESSERACT = True
except Exception:
    _HAS_TESSERACT = False


def extract_pdf(
    content: bytes,
    upload_id: str,
    filename: str,
    max_pages: int = 500,
) -> ExtractedPdf:
    if not _HAS_PYMUPDF:
        raise RuntimeError("PyMuPDF (pymupdf) is required for PDF extraction")

    doc = fitz.open(stream=content, filetype="pdf")  # type: ignore[attr-defined]
    pages: list[PageContent] = []
    ocr_confidences: list[float] = []

    total = min(len(doc), max_pages)
    for page_num in range(total):
        page = doc[page_num]
        text = page.get_text("text").strip()
        confidence: float | None = None

        if len(text) < _NATIVE_CHAR_MIN and _HAS_TESSERACT:
            pix = page.get_pixmap(dpi=150)
            img = Image.open(io.BytesIO(pix.tobytes("png")))
            data = pytesseract.image_to_data(
                img,
                output_type=pytesseract.Output.DICT,
                lang="por+eng",
            )
            conf_vals = [
                int(c) for c in data["conf"] if str(c).isdigit() and int(c) > 0
            ]
            if conf_vals:
                confidence = sum(conf_vals) / len(conf_vals)
                ocr_confidences.append(confidence)
            ocr_words = [
                w
                for w, c in zip(data["text"], data["conf"])
                if str(c).isdigit() and int(c) > 50
            ]
            text = " ".join(ocr_words).strip() or text

        pages.append(
            PageContent(
                page_number=page_num + 1,
                text=text,
                ocr_confidence=confidence,
                is_chart_heavy=len(text) < _CHART_HEAVY_THRESHOLD,
            )
        )

    doc.close()
    avg_conf = sum(ocr_confidences) / len(ocr_confidences) if ocr_confidences else None
    return ExtractedPdf(
        upload_id=upload_id,
        filename=filename,
        pages=pages,
        avg_ocr_confidence=avg_conf,
    )
