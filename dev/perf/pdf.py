#!/usr/bin/env python3
"""Build a narrated, human-readable PDF report for a performance run.

    python3 dev/perf/pdf.py results/perf
"""

import csv
import html
import json
import os
import re
import sys
from pathlib import Path

try:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    HAVE_MATPLOTLIB = True
except ImportError:
    HAVE_MATPLOTLIB = False

try:
    from reportlab.lib.colors import HexColor
    from reportlab.lib.pagesizes import A4
    from reportlab.lib.styles import ParagraphStyle, getSampleStyleSheet
    from reportlab.pdfbase.pdfmetrics import stringWidth
    from reportlab.pdfgen import canvas
    from reportlab.platypus import (
        Flowable,
        HRFlowable,
        Image,
        KeepTogether,
        PageBreak,
        Paragraph,
        SimpleDocTemplate,
        Spacer,
        Table,
        TableStyle,
    )
    from reportlab.graphics.shapes import (
        Circle, Ellipse,
        Drawing,
        Group,
        Line,
        Polygon,
        Rect,
        String,
    )
    HAVE_REPORTLAB = True
except ImportError:
    HAVE_REPORTLAB = False


PAGE_WIDTH, PAGE_HEIGHT = A4 if HAVE_REPORTLAB else (595.27, 841.89)
MARGIN = 40.0
CONTENT_WIDTH = PAGE_WIDTH - 2 * MARGIN

CLR_PRIMARY = HexColor("#1a365d") if HAVE_REPORTLAB else None
CLR_SECONDARY = HexColor("#2b6cb0") if HAVE_REPORTLAB else None
CLR_TEXT = HexColor("#2d3748") if HAVE_REPORTLAB else None
CLR_MUTED = HexColor("#718096") if HAVE_REPORTLAB else None
CLR_BORDER = HexColor("#cbd5e0") if HAVE_REPORTLAB else None


class Bookmark(Flowable if HAVE_REPORTLAB else object):
    """Marks a page destination and adds a PDF outline entry."""

    def __init__(self, key: str, title: str):
        super().__init__()
        self.key = key
        self.title = title

    def wrap(self, availWidth, availHeight):
        return 0, 0

    def draw(self):
        self.canv.bookmarkPage(self.key)
        self.canv.addOutlineEntry(self.title, self.key, level=0, closed=False)


class NumberedCanvas(canvas.Canvas if HAVE_REPORTLAB else object):
    """Canvas that computes total pages dynamically for the footer."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._saved_page_states = []
        self.commit_sha = ""

    def showPage(self):
        self._saved_page_states.append(dict(self.__dict__))
        self._startPage()

    def save(self):
        num_pages = len(self._saved_page_states)
        for state in self._saved_page_states:
            self.__dict__.update(state)
            self.draw_footer(num_pages)
            super().showPage()
        super().save()

    def draw_footer(self, total_pages):
        self.saveState()
        self.setFont("Helvetica", 8)
        self.setFillColor(CLR_MUTED)
        self.setStrokeColor(HexColor("#e2e8f0"))
        self.setLineWidth(0.5)
        self.line(MARGIN, 30, PAGE_WIDTH - MARGIN, 30)

        sha = self.commit_sha or "unknown"
        txt = f"Antares perf report · commit {sha} · page {self._pageNumber} of {total_pages}"
        self.drawRightString(PAGE_WIDTH - MARGIN, 20, txt)
        self.drawString(MARGIN, 20, "ETSI CIM 009 NGSI-LD Performance Verification")
        self.restoreState()


def md_table(text: str) -> list[list[str]]:
    """Parse pipe table text into rows of cell strings."""
    rows = []
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("|") and not set(line) <= set("|-: "):
            parts = [c.strip() for c in line.strip("|").split("|")]
            rows.append(parts)
    return rows


def parse_float(val: any) -> float | None:
    if val is None:
        return None
    s = str(val).replace(",", "").replace(" ", "").strip()
    m = re.search(r"[-+]?[0-9]*\.?[0-9]+", s)
    if m:
        try:
            return float(m.group(0))
        except ValueError:
            return None
    return None


def parse_int(val: any) -> int | None:
    f = parse_float(val)
    return int(round(f)) if f is not None else None


def fmt_int(n: int | float | None) -> str:
    if n is None:
        return "—"
    return f"{int(round(n)):,}".replace(",", " ")


def fmt_ms(v: float | None) -> str:
    if v is None:
        return "—"
    return f"{v:.1f} ms"


def col(rows: list[list[str]], name: str) -> list[str]:
    """Return column values by header name substring."""
    if not rows or len(rows) < 2:
        return []
    head = [h.strip().lower() for h in rows[0]]
    needle = name.strip().lower()
    idx = -1
    for i, h in enumerate(head):
        if needle in h:
            idx = i
            break
    if idx == -1:
        return []
    return [r[idx] if idx < len(r) else "" for r in rows[1:]]


def make_box(x, y, w, h, title, subtitle=None, bg="#edf2f7", stroke="#cbd5e0"):
    g = Group()
    g.add(Rect(x, y, w, h, rx=4, ry=4, fillColor=HexColor(bg), strokeColor=HexColor(stroke), strokeWidth=1))
    mid_y = y + (h / 2 + 3 if subtitle else h / 2 - 3)
    g.add(String(x + w / 2, mid_y, title, textAnchor="middle", fontName="Helvetica-Bold", fontSize=8.5, fillColor=HexColor("#1a202c")))
    if subtitle:
        g.add(String(x + w / 2, y + h / 2 - 8, subtitle, textAnchor="middle", fontName="Helvetica", fontSize=7, fillColor=HexColor("#4a5568")))
    return g


def make_arrow(x1, y1, x2, y2, label=None, label_side="top"):
    g = Group()
    g.add(Line(x1, y1, x2, y2, strokeColor=HexColor("#4a5568"), strokeWidth=1.2))
    dx, dy = x2 - x1, y2 - y1
    length = (dx * dx + dy * dy) ** 0.5
    if length > 0:
        ux, uy = dx / length, dy / length
        px, py = -uy, ux
        ah = 5
        g.add(Polygon([x2, y2, x2 - ah * ux + ah * 0.5 * px, y2 - ah * uy + ah * 0.5 * py,
                       x2 - ah * ux - ah * 0.5 * px, y2 - ah * uy - ah * 0.5 * py],
                      fillColor=HexColor("#4a5568"), strokeColor=HexColor("#4a5568")))
    if label:
        mx = (x1 + x2) / 2
        my = (y1 + y2) / 2 + (4 if label_side == "top" else -10)
        anchor = "middle"
        if label_side == "right":
            mx, my, anchor = mx + 5, (y1 + y2) / 2 - 2, "start"
        g.add(String(mx, my, label, textAnchor=anchor, fontName="Helvetica", fontSize=7, fillColor=HexColor("#4a5568")))
    return g


def make_cylinder(x, y, w, h, title):
    g = Group()
    rx, ry = w / 2, 5
    g.add(Rect(x, y + ry, w, h - 2 * ry, fillColor=HexColor("#edf2f7"), strokeColor=HexColor("#edf2f7"), strokeWidth=0))
    g.add(Line(x, y + ry, x, y + h - ry, strokeColor=HexColor("#cbd5e0"), strokeWidth=1))
    g.add(Line(x + w, y + ry, x + w, y + h - ry, strokeColor=HexColor("#cbd5e0"), strokeWidth=1))
    g.add(Ellipse(x + rx, y + ry, rx, ry, fillColor=HexColor("#edf2f7"), strokeColor=HexColor("#cbd5e0"), strokeWidth=1))
    g.add(Ellipse(x + rx, y + h - ry, rx, ry, fillColor=HexColor("#e2e8f0"), strokeColor=HexColor("#cbd5e0"), strokeWidth=1))
    g.add(String(x + rx, y + h / 2 - 3, title, textAnchor="middle", fontName="Helvetica-Bold", fontSize=7.5, fillColor=HexColor("#1a202c")))
    return g


def make_cloud(x, y, w, h, title):
    g = Group()
    g.add(Rect(x, y, w, h, rx=8, ry=8, fillColor=HexColor("#fefcbf"), strokeColor=HexColor("#ecc94b"), strokeWidth=1))
    g.add(String(x + w / 2, y + h / 2 - 3, title, textAnchor="middle", fontName="Helvetica-Bold", fontSize=7.5, fillColor=HexColor("#744210")))
    return g


def diagram_system() -> Drawing:
    d = Drawing(CONTENT_WIDTH, 118)
    d.add(Rect(0, 0, CONTENT_WIDTH, 118, rx=5, ry=5, fillColor=HexColor("#f8fafc"), strokeColor=HexColor("#e2e8f0"), strokeWidth=0.8))
    d.add(String(10, 104, "One rented machine: everything below shares its cores and memory", fontName="Helvetica-Bold", fontSize=8, fillColor=HexColor("#718096")))
    d.add(make_box(15, 50, 85, 42, "k6", "load generator", bg="#e6fffa", stroke="#81e6d9"))
    d.add(make_box(150, 50, 100, 42, "Antares broker", "NGSI-LD API", bg="#ebf8ff", stroke="#90cdf4"))
    d.add(make_cylinder(165, 4, 70, 34, "PostgreSQL"))
    d.add(make_box(345, 66, 100, 26, "HTTP sink", "receives notifications", bg="#fefcbf", stroke="#ecc94b"))
    d.add(make_box(345, 34, 100, 26, "Mosquitto", "MQTT broker", bg="#feebc8", stroke="#fbd38d"))
    d.add(make_arrow(100, 71, 150, 71, "HTTP requests"))
    d.add(make_arrow(200, 50, 200, 38, "SQL", "right"))
    d.add(make_arrow(250, 79, 345, 79, "notifications, federated calls"))
    d.add(make_arrow(250, 61, 345, 47, "MQTT notifications", "bottom"))
    return d


def diagram_dataset() -> Drawing:
    d = Drawing(CONTENT_WIDTH, 75)
    d.add(Rect(0, 0, CONTENT_WIDTH, 75, rx=5, ry=5, fillColor=HexColor("#f8fafc"), strokeColor=HexColor("#e2e8f0"), strokeWidth=0.8))
    d.add(make_box(15, 14, 135, 46, "Entities", "Vehicle · Building · Sensor", bg="#e6fffa", stroke="#81e6d9"))
    d.add(make_box(180, 14, 135, 46, "Subscriptions", "8 filter classes", bg="#ebf8ff", stroke="#90cdf4"))
    d.add(make_box(345, 14, 145, 46, "Registrations", "8 registration classes", bg="#fefcbf", stroke="#ecc94b"))
    d.add(make_arrow(150, 37, 180, 37, "evaluated by"))
    d.add(make_arrow(315, 37, 345, 37, "indexed by"))
    return d


def diagram_shapes() -> Drawing:
    d = Drawing(CONTENT_WIDTH, 70)
    d.add(Rect(0, 0, CONTENT_WIDTH, 70, rx=5, ry=5, fillColor=HexColor("#f8fafc"), strokeColor=HexColor("#e2e8f0"), strokeWidth=0.8))
    d.add(make_box(15, 14, 110, 42, "Closed-Loop Clients", "c50 / c200 VUs", bg="#e6fffa", stroke="#81e6d9"))
    d.add(make_box(165, 38, 145, 24, "Query: limit 20 vehicles", "GET /entities?type=...", bg="#ebf8ff", stroke="#90cdf4"))
    d.add(make_box(165, 8, 145, 24, "Retrieve: single entity", "GET /entities/{id}", bg="#ebf8ff", stroke="#90cdf4"))
    d.add(make_box(350, 14, 115, 42, "Storage Driver", "PostgreSQL / memory", bg="#edf2f7", stroke="#cbd5e0"))
    d.add(make_arrow(125, 42, 165, 50))
    d.add(make_arrow(125, 28, 165, 20))
    d.add(make_arrow(310, 50, 350, 42))
    d.add(make_arrow(310, 20, 350, 28))
    return d


def diagram_saturate() -> Drawing:
    d = Drawing(CONTENT_WIDTH, 75)
    d.add(Rect(0, 0, CONTENT_WIDTH, 75, rx=5, ry=5, fillColor=HexColor("#f8fafc"), strokeColor=HexColor("#e2e8f0"), strokeWidth=0.8))
    x0, y0 = 35, 14
    for i in range(5):
        d.add(Rect(x0 + i * 36, y0, 36, (i + 1) * 9, fillColor=HexColor("#90cdf4"), strokeColor=HexColor("#63b3ed"), strokeWidth=0.8))
        d.add(String(x0 + i * 36 + 18, y0 + (i + 1) * 9 + 3, f"{(i+1)*500}", fontName="Helvetica", fontSize=6, textAnchor="middle"))
    d.add(Line(25, y0 + 35, 225, y0 + 35, strokeColor=HexColor("#e53e3e"), strokeWidth=1.2, strokeDashArray=[3, 3]))
    d.add(String(230, y0 + 32, "p99 50 ms & 0.1% error threshold", fontName="Helvetica-Bold", fontSize=7, fillColor=HexColor("#e53e3e")))
    d.add(make_box(340, 14, 130, 44, "Saturation Knee", "last sustainable stage", bg="#feebc8", stroke="#fbd38d"))
    d.add(make_arrow(215, y0 + 18, 340, 36, "identifies"))
    return d


def diagram_fire() -> Drawing:
    d = Drawing(CONTENT_WIDTH, 75)
    d.add(Rect(0, 0, CONTENT_WIDTH, 75, rx=5, ry=5, fillColor=HexColor("#f8fafc"), strokeColor=HexColor("#e2e8f0"), strokeWidth=0.8))
    d.add(make_box(10, 16, 95, 42, "1. Write Stream", "PATCH /attrs", bg="#e6fffa", stroke="#81e6d9"))
    d.add(make_box(130, 16, 95, 42, "2. Matcher", "SubMirror index", bg="#ebf8ff", stroke="#90cdf4"))
    d.add(make_box(250, 16, 105, 42, "3. Change Queue", "bounded buffer (1024)", bg="#fefcbf", stroke="#ecc94b"))
    d.add(make_box(380, 16, 95, 42, "4. Delivery", "POST /n/k to sink", bg="#feebc8", stroke="#fbd38d"))
    d.add(make_arrow(105, 37, 130, 37))
    d.add(make_arrow(225, 37, 250, 37))
    d.add(make_arrow(355, 37, 380, 37))
    return d


def diagram_fed() -> Drawing:
    d = Drawing(CONTENT_WIDTH, 75)
    d.add(Rect(0, 0, CONTENT_WIDTH, 75, rx=5, ry=5, fillColor=HexColor("#f8fafc"), strokeColor=HexColor("#e2e8f0"), strokeWidth=0.8))
    d.add(make_box(10, 16, 90, 42, "Query", "GET /entities", bg="#e6fffa", stroke="#81e6d9"))
    d.add(make_box(125, 16, 100, 42, "Registry Match", "csource_index", bg="#ebf8ff", stroke="#90cdf4"))
    d.add(make_cloud(250, 40, 95, 24, "Source A (/csr/1)"))
    d.add(make_cloud(250, 8, 95, 24, "Source N (/csr/N)"))
    d.add(make_box(375, 16, 95, 42, "Merged Result", "assembled answer", bg="#e2e8f0", stroke="#cbd5e0"))
    d.add(make_arrow(100, 37, 125, 37))
    d.add(make_arrow(225, 42, 250, 52, "fan-out"))
    d.add(make_arrow(225, 30, 250, 20, "fan-out"))
    d.add(make_arrow(345, 52, 375, 42))
    d.add(make_arrow(345, 20, 375, 30))
    return d


def chart_fire(out_path: Path, fire_rows: list[list[str]]) -> tuple[str | None, str | None]:
    if not HAVE_MATPLOTLIB or len(fire_rows) < 2:
        return None, None
    rates = [parse_float(x) for x in col(fire_rows, "rate")]
    pcts = [parse_float(x) for x in col(fire_rows, "delivered %")]
    drops = [parse_float(x) for x in col(fire_rows, "dropped")]
    p99_patch = [parse_float(x) for x in col(fire_rows, "PATCH p99")]
    p99_get = [parse_float(x) for x in col(fire_rows, "GET p99")]

    valid = [(r, p, d) for r, p, d in zip(rates, pcts, drops) if r is not None and p is not None]
    if not valid:
        return None, None

    r_vals = [v[0] for v in valid]
    p_vals = [v[1] for v in valid]
    d_vals = [v[2] or 0.0 for v in valid]

    fig, ax1 = plt.subplots(figsize=(6.8, 2.5), dpi=130)
    color1 = "#2b6cb0"
    ax1.set_xlabel("Update arrival rate (updates/s)", fontsize=8)
    ax1.set_ylabel("Notifications delivered (%)", color=color1, fontsize=8)
    line1 = ax1.plot(r_vals, p_vals, color=color1, marker="o", linewidth=1.5, label="Delivered %")
    ax1.tick_params(axis="y", labelcolor=color1, labelsize=7)
    ax1.tick_params(axis="x", labelsize=7)
    ax1.set_ylim(-5, 105)
    ax1.grid(True, linestyle="--", alpha=0.4)

    ax2 = ax1.twinx()
    color2 = "#e53e3e"
    ax2.set_ylabel("Dropped changes count", color=color2, fontsize=8)
    line2 = ax2.plot(r_vals, d_vals, color=color2, marker="s", linestyle="--", linewidth=1.2, label="Dropped changes")
    ax2.tick_params(axis="y", labelcolor=color2, labelsize=7)

    lines = line1 + line2
    labels = [l.get_label() for l in lines]
    ax1.legend(lines, labels, loc="center left", fontsize=7)
    fig.tight_layout()
    chart1_path = out_path / "fire-delivery.png"
    fig.savefig(chart1_path)
    plt.close(fig)

    lat_valid = [(r, p, g) for r, p, g in zip(rates, p99_patch, p99_get) if r is not None and (p is not None or g is not None)]
    chart2_path = None
    if lat_valid:
        fig2, ax = plt.subplots(figsize=(6.8, 2.4), dpi=130)
        lr = [v[0] for v in lat_valid]
        lp = [v[1] for v in lat_valid]
        lg = [v[2] for v in lat_valid]
        if any(lp):
            ax.plot(lr, lp, marker="o", color="#319795", label="PATCH p99 (ms)", linewidth=1.4)
        if any(lg):
            ax.plot(lr, lg, marker="^", color="#d69e2e", label="GET p99 (ms)", linewidth=1.4)
        ax.set_xlabel("Update arrival rate (updates/s)", fontsize=8)
        ax.set_ylabel("Latency p99 (ms)", fontsize=8)
        ax.tick_params(labelsize=7)
        ax.grid(True, linestyle="--", alpha=0.4)
        ax.legend(loc="upper left", fontsize=7)
        fig2.tight_layout()
        chart2_path = out_path / "fire-latency.png"
        fig2.savefig(chart2_path)
        plt.close(fig2)

    return str(chart1_path), str(chart2_path) if chart2_path else None


def chart_fed(out_path: Path, fed_rows: list[list[str]]) -> str | None:
    if not HAVE_MATPLOTLIB or len(fed_rows) < 2:
        return None
    rates = [parse_float(x) for x in col(fed_rows, "rate")]
    p99s = [parse_float(x) for x in col(fed_rows, "p99")]
    calls = [parse_float(x) for x in col(fed_rows, "calls per query")]

    valid = [(r, p, c) for r, p, c in zip(rates, p99s, calls) if r is not None and p is not None]
    if not valid:
        return None

    fig, ax1 = plt.subplots(figsize=(6.8, 2.5), dpi=130)
    r_vals = [v[0] for v in valid]
    p_vals = [v[1] for v in valid]
    c_vals = [v[2] or 0.0 for v in valid]

    color1 = "#805ad5"
    ax1.set_xlabel("Query arrival rate (queries/s)", fontsize=8)
    ax1.set_ylabel("GET p99 latency (ms)", color=color1, fontsize=8)
    line1 = ax1.plot(r_vals, p_vals, color=color1, marker="o", linewidth=1.5, label="GET p99 (ms)")
    ax1.tick_params(axis="y", labelcolor=color1, labelsize=7)
    ax1.tick_params(axis="x", labelsize=7)
    ax1.grid(True, linestyle="--", alpha=0.4)

    ax2 = ax1.twinx()
    color2 = "#dd6b20"
    ax2.set_ylabel("Sources dialled per query", color=color2, fontsize=8)
    line2 = ax2.plot(r_vals, c_vals, color=color2, marker="x", linestyle="--", linewidth=1.2, label="Calls per query")
    ax2.tick_params(axis="y", labelcolor=color2, labelsize=7)

    lines = line1 + line2
    labels = [l.get_label() for l in lines]
    ax1.legend(lines, labels, loc="upper left", fontsize=7)
    fig.tight_layout()
    p = out_path / "fed-chart.png"
    fig.savefig(p)
    plt.close(fig)
    return str(p)


def chart_rss(out_path: Path, csv_path: Path) -> str | None:
    if not HAVE_MATPLOTLIB or not csv_path.exists():
        return None
    rows = list(csv.DictReader(open(csv_path)))
    if not rows:
        return None
    t0 = int(rows[0]["t"])
    t = [(int(r["t"]) - t0) / 60 for r in rows]
    num = lambda k: [float(r.get(k) or 0) for r in rows]

    cores = int(float(rows[0].get("host_cores") or 0))
    fig, ax = plt.subplots(figsize=(6.8, 2.5), dpi=130)
    ax.fill_between(t, num("host_busy_cores"), color="#e2e8f0", lw=0, label="Host busy cores")
    ax.plot(t, [v / 100 for v in num("broker_cpu_pct")], color="#1f77b4", lw=1.2, label="Broker CPU")
    ax.plot(t, [v / 100 for v in num("postgres_cpu_pct")], color="#d62728", lw=1.2, label="PostgreSQL CPU")
    if cores > 0:
        ax.axhline(cores, ls="--", lw=0.8, color="#718096", label=f"Host ceiling ({cores} cores)")
        ax.set_ylim(0, max(cores * 1.1, max(num("host_busy_cores") or [1])))

    ax.set_xlabel("Minutes since sampler start", fontsize=8)
    ax.set_ylabel("Cores busy", fontsize=8)
    ax.tick_params(labelsize=7)
    ax.legend(loc="upper left", fontsize=7)
    fig.tight_layout()
    target = out_path / "rss-chart-gen.png"
    fig.savefig(target)
    plt.close(fig)
    return str(target)


def build_table(rows: list[list[any]], col_widths=None) -> Table:
    if not rows:
        return Table([["No data"]])
    pstyle_th = ParagraphStyle("TH", fontName="Helvetica-Bold", fontSize=7.5, leading=9.5, textColor=HexColor("#1a202c"))
    pstyle_td = ParagraphStyle("TD", fontName="Helvetica", fontSize=7.5, leading=9.5, textColor=HexColor("#2d3748"))
    pstyle_code = ParagraphStyle("TCode", fontName="Courier", fontSize=6.5, leading=8, textColor=HexColor("#2d3748"))

    formatted = []
    for r_idx, r in enumerate(rows):
        row_cells = []
        is_th = (r_idx == 0)
        for c in r:
            if isinstance(c, Flowable):
                row_cells.append(c)
            else:
                txt = str(c)
                if not is_th and ("{" in txt or "[" in txt):
                    st = pstyle_code
                else:
                    st = pstyle_th if is_th else pstyle_td
                row_cells.append(Paragraph(txt, st))
        formatted.append(row_cells)

    tbl = Table(formatted, colWidths=col_widths, repeatRows=1)
    tbl.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, 0), HexColor("#edf2f7")),
        ("ALIGN", (0, 0), (-1, -1), "LEFT"),
        ("VALIGN", (0, 0), (-1, -1), "TOP"),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 3),
        ("TOPPADDING", (0, 0), (-1, -1), 3),
        ("LEFTPADDING", (0, 0), (-1, -1), 4),
        ("RIGHTPADDING", (0, 0), (-1, -1), 4),
        ("LINEBELOW", (0, 0), (-1, 0), 1, HexColor("#cbd5e0")),
        ("LINEBELOW", (0, 1), (-1, -1), 0.5, HexColor("#e2e8f0")),
    ]))
    return tbl


def build(out: str, record: dict) -> str | None:
    """Build the narrated PDF report in directory `out`."""
    if not HAVE_REPORTLAB or not HAVE_MATPLOTLIB:
        print("reportlab and matplotlib are required for PDF report generation; skipping report.pdf")
        return None

    out_path = Path(out)
    out_path.mkdir(parents=True, exist_ok=True)
    pdf_filename = out_path / "report.pdf"

    commit_sha = record.get("commit", "unknown")
    host_str = record.get("host", "dedicated runner")
    tables = record.get("tables", {})

    styles = getSampleStyleSheet()
    h1 = ParagraphStyle("RptH1", fontName="Helvetica-Bold", fontSize=16, leading=20, textColor=CLR_PRIMARY, spaceAfter=6)
    h2 = ParagraphStyle("RptH2", fontName="Helvetica-Bold", fontSize=11, leading=15, textColor=CLR_SECONDARY, spaceBefore=8, spaceAfter=4)
    h3 = ParagraphStyle("RptH3", fontName="Helvetica-Bold", fontSize=9, leading=12, textColor=CLR_TEXT, spaceBefore=5, spaceAfter=2)
    body = ParagraphStyle("RptBody", fontName="Helvetica", fontSize=8.5, leading=12, textColor=CLR_TEXT, spaceAfter=5)
    caption = ParagraphStyle("RptCaption", fontName="Helvetica-Oblique", fontSize=7.5, leading=10, textColor=CLR_MUTED, spaceAfter=6, alignment=1)
    callout = ParagraphStyle("RptCallout", fontName="Helvetica", fontSize=8, leading=11, textColor=CLR_PRIMARY)

    load_text = ""
    load_md_path = out_path / "load.md"
    if load_md_path.exists():
        load_text = open(load_md_path).read()

    # Pre-parse metrics for cover tiles
    ent_count = None
    load_counts = {}
    if load_text:
        m_ent = re.search(r"(\d+)\s+entities", load_text)
        if m_ent:
            ent_count = int(m_ent.group(1))
        for key in ("tenants", "subscriptions", "registrations"):
            m = re.search(r"(\d+)\s+" + key, load_text)
            if m:
                load_counts[key] = int(m.group(1))

    def counted(key: str, noun: str) -> str:
        n = load_counts.get(key)
        return f"{fmt_int(n)} {noun}" if n is not None else f"every {noun.split()[-1]}"

    fire_rows = tables.get("fire", [])
    safe_rate = None
    first_bad_rate = None
    first_bad_drop = None
    first_bad_p99 = None
    if fire_rows:
        rates = col(fire_rows, "rate")
        pcts = col(fire_rows, "delivered %")
        failed_ops = col(fire_rows, "failed")
        drops_col = col(fire_rows, "dropped")
        p99_col = col(fire_rows, "PATCH p99")
        for r, p, f, d, lp in zip(rates, pcts, failed_ops, drops_col, p99_col):
            pv = parse_float(p)
            err_cnt = parse_int(f.split()[0]) if f else None
            if pv is not None and pv >= 99.0 and (err_cnt == 0 or "0 (0/0/0)" in f):
                safe_rate = parse_int(r)
            elif safe_rate is not None and first_bad_rate is None:
                first_bad_rate = parse_int(r)
                first_bad_drop = parse_int(d)
                first_bad_p99 = parse_float(lp)

    shapes_rows = tables.get("shapes", [])
    q_rps_pg = None
    q_p99_pg = None
    ret_rps_pg = None
    ret_p99_pg = None
    q_rps_mem = None
    if shapes_rows:
        for r in shapes_rows[1:]:
            st = r[0].lower() if len(r) > 0 else ""
            sh = r[1].lower() if len(r) > 1 else ""
            c = r[2].lower() if len(r) > 2 else ""
            if "postgres" in st and "query" in sh and "c50" in c:
                q_rps_pg = parse_int(r[3])
                q_p99_pg = parse_float(r[4])
            if "postgres" in st and "retrieve" in sh and "c50" in c:
                ret_rps_pg = parse_int(r[3])
                ret_p99_pg = parse_float(r[4])
            if "memory" in st and "query" in sh and "c50" in c:
                q_rps_mem = parse_int(r[3])

    rss_rows = tables.get("rss", [])
    b_rss = None
    p_rss = None
    b_cpu_peak = None
    p_cpu_peak = None
    h_cpu_peak = None
    h_cores_max = None
    if rss_rows:
        for r in rss_rows:
            label = r[0].lower() if len(r) > 0 else ""
            val = r[1] if len(r) > 1 else ""
            ext = r[2] if len(r) > 2 else ""
            if "broker rss peak" in label:
                b_rss = val
            elif "postgres rss peak" in label:
                p_rss = val
            elif "broker cpu peak" in label:
                b_cpu_peak = val
            elif "postgres cpu peak" in label:
                p_cpu_peak = val
            elif "host busy peak" in label:
                h_cpu_peak = val
                m_cores = re.search(r"of\s+(\d+)", ext)
                if m_cores:
                    h_cores_max = parse_int(m_cores.group(1))

    fed_rows = tables.get("fed", [])
    fed_calls = None
    if len(fed_rows) > 1 and len(fed_rows[1]) >= 7:
        fed_calls = parse_float(fed_rows[1][6])

    story = []
    emitted_sections = []

    def add_section_header(title: str, anchor_id: str):
        emitted_sections.append((title, anchor_id))
        story.append(Bookmark(anchor_id, title))
        story.append(Paragraph(f'<a name="{anchor_id}"/>{title}', h1))
        story.append(HRFlowable(width="100%", thickness=1, color=CLR_SECONDARY, spaceAfter=6))

    def sub_header(title: str):
        story.append(Paragraph(f"<b>{title}</b>", h3))

    # COVER PAGE
    story.append(Spacer(1, 10))
    story.append(Paragraph("Antares Context Broker", ParagraphStyle("CoverSub", fontName="Helvetica-Bold", fontSize=13, leading=16, textColor=CLR_SECONDARY)))
    story.append(Paragraph("Performance and Scale Report", ParagraphStyle("CoverTitle", fontName="Helvetica-Bold", fontSize=22, leading=26, textColor=CLR_PRIMARY, spaceAfter=8)))
    story.append(Paragraph(f"Commit: <b>{commit_sha}</b> &nbsp;&nbsp;|&nbsp;&nbsp; Host: <b>{host_str}</b>", body))
    story.append(HRFlowable(width="100%", thickness=1.5, color=CLR_PRIMARY, spaceAfter=12))

    # Build headline tiles dynamically
    tiles_cells = []
    if ent_count is not None:
        tiles_cells.append(Paragraph(f"<b>Entities Stored</b><br/><font size=13 color='#1a365d'><b>{fmt_int(ent_count)}</b></font>", callout))
    if safe_rate is not None:
        tiles_cells.append(Paragraph(f"<b>Notification Limit</b><br/><font size=13 color='#1a365d'><b>{fmt_int(safe_rate)} updates/s</b></font>", callout))
    if q_rps_pg is not None:
        tiles_cells.append(Paragraph(f"<b>Query (c50 PG)</b><br/><font size=13 color='#1a365d'><b>{fmt_int(q_rps_pg)} req/s</b></font>", callout))
    if b_rss is not None:
        tiles_cells.append(Paragraph(f"<b>Broker RSS Peak</b><br/><font size=13 color='#1a365d'><b>{b_rss}</b></font>", callout))
    if fed_calls is not None:
        tiles_cells.append(Paragraph(f"<b>Federated Fan-out</b><br/><font size=13 color='#1a365d'><b>{fed_calls:.1f} / query</b></font>", callout))

    if tiles_cells:
        w_tile = CONTENT_WIDTH / len(tiles_cells)
        t_tiles = Table([tiles_cells], colWidths=[w_tile] * len(tiles_cells))
        t_tiles.setStyle(TableStyle([
            ("BACKGROUND", (0, 0), (-1, -1), HexColor("#edf2f7")),
            ("BOX", (0, 0), (-1, -1), 1, HexColor("#cbd5e0")),
            ("INNERGRID", (0, 0), (-1, -1), 0.5, HexColor("#cbd5e0")),
            ("TOPPADDING", (0, 0), (-1, -1), 6),
            ("BOTTOMPADDING", (0, 0), (-1, -1), 6),
            ("ALIGN", (0, 0), (-1, -1), "CENTER"),
        ]))
        story.append(t_tiles)
        story.append(Spacer(1, 10))

    toc_marker_idx = len(story)

    # SECTION 1: HOW TO READ THIS REPORT
    sec1_story = [PageBreak()]
    sec1_story.append(Bookmark("sec_intro", "1. How to Read This Report"))
    emitted_sections.append(("1. How to Read This Report", "sec_intro"))
    sec1_story.append(Paragraph('<a name="sec_intro"/>1. How to Read This Report', h1))
    sec1_story.append(HRFlowable(width="100%", thickness=1, color=CLR_SECONDARY, spaceAfter=6))
    sec1_story.append(Paragraph(
        "A Context Broker is an ETSI CIM 009 standard component that maintains the state of a system. "
        "Clients create <b>Entities</b> (vehicles, buildings, sensors) with typed <b>Attributes</b>. "
        "Applications register <b>Subscriptions</b> to receive push notifications when changes match filters. "
        "Brokers delegate queries to other context sources through <b>Context Source Registrations</b>.",
        body
    ))
    sec1_story.append(Paragraph(
        f"<b>Test Machine Environment:</b> Tests executed on an isolated single host (<code>{host_str}</code>). "
        "The test rig co-locates the Antares broker, a PostgreSQL 17 database with PostGIS, the k6 load generator, "
        "the mock receiver sink, and the Mosquitto MQTT broker.",
        body
    ))
    sec1_story.append(Spacer(1, 3))
    sec1_story.append(diagram_system())
    sec1_story.append(Paragraph("Figure 1.1: Rig topology and inter-service communication.", caption))
    sec1_story.append(Paragraph("<b>Core Benchmarking Terminology</b>", h3))
    glossary = [
        ["req/s (Throughput)", "Requests completed per second by the broker."],
        ["p99 Latency", "99th percentile response time: 99 of 100 requests answered within this window."],
        ["RSS (Resident Memory)", "Resident Set Size: physical RAM occupied by the process."],
        ["Closed-Loop Load", "Fixed client concurrency (e.g. c50): each client waits for a response before firing the next."],
        ["Open-Loop Load", "Fixed arrival rate: requests fire on an exact schedule regardless of completion time."],
        ["Tenant", "Logical data partition using PostgreSQL Row-Level Security (RLS)."],
        ["Saturation Knee", "Maximum sustainable arrival rate before response latency or error rate exceeds threshold."],
    ]
    glossary_table = build_table([[Paragraph(f"<b>{k}</b>", body), Paragraph(v, body)] for k, v in glossary], col_widths=[140, CONTENT_WIDTH - 140])
    sec1_story.append(glossary_table)
    story.extend(sec1_story)

    # SECTION 2: WHAT WAS STORED IN THE BROKER
    subs_rows = tables.get("subs", [])
    csr_rows = tables.get("csr", [])
    if load_text or subs_rows or csr_rows:
        story.append(PageBreak())
        add_section_header("2. Dataset Composition and Initial Load", "sec_load")
        sub_header("What was measured")
        story.append(Paragraph(
            "Initial data population wall times and entity distributions across multi-tenant partitions. "
            "Entities are loaded directly into PostgreSQL via bulk streaming COPY to seed the system, "
            "while Subscriptions and Context Source Registrations pass through the API to validate and index.",
            body
        ))
        sub_header("Under which conditions")
        tenant_count_str = "multiple"
        m_ten = re.search(r"(\d+)\s+tenants", load_text)
        if m_ten:
            tenant_count_str = m_ten.group(1)
        story.append(Paragraph(
            f"Entities partitioned across {tenant_count_str} distinct tenants (t0, t1, ...). "
            "Entities belong to three standard categories: Vehicle (speed, brand, location), Building (wide multi-property), "
            "and Sensor (metadata payload). Subscriptions and registrations loaded via REST API.",
            body
        ))
        story.append(diagram_dataset())
        story.append(Paragraph("Figure 2.1: Dataset distribution across tenants, subscriptions, and registrations.", caption))

        sub_header("The numbers")
        load_summary_rows = [["Component", "Target Count", "Load Time", "Effective Ingestion Rate"]]
        if load_text:
            for l in [line.strip() for line in load_text.splitlines() if line.strip()]:
                if l.startswith("-"):
                    m = re.search(r"-\s*([^(:]+)(?:\(([^)]+)\))?:\s*(\d+)\s*s", l)
                    if m:
                        comp = m.group(1).strip()
                        cnt_str = m.group(2) or ""
                        sec = float(m.group(3))
                        cnt = parse_float(cnt_str) or 0
                        rate_str = f"{cnt/sec:,.0f} items/s".replace(",", " ") if sec > 0 and cnt > 0 else "—"
                        load_summary_rows.append([comp, cnt_str, f"{sec:.0f} s", rate_str])
        if len(load_summary_rows) > 1:
            story.append(build_table(load_summary_rows, col_widths=[140, 110, 90, 130]))
            story.append(Spacer(1, 4))

        if subs_rows and len(subs_rows) > 1:
            story.append(Paragraph("<b>Subscription Classes</b>", h3))
            plain_subs = [["Class", "Watched Entities", "Filter Rule", "Trigger Condition", "Count"]]
            for r in subs_rows[1:]:
                if len(r) >= 5:
                    plain_subs.append([r[0], r[1], r[2], r[3], r[4]])
            story.append(build_table(plain_subs, col_widths=[90, 90, 120, 125, 45]))
            story.append(Spacer(1, 4))

        if csr_rows and len(csr_rows) > 1:
            story.append(Paragraph("<b>Context Source Registration Classes</b>", h3))
            plain_csr = [["Class", "Type", "Mode and Operations", "Parameters", "Count"]]
            for r in csr_rows[1:]:
                if len(r) >= 5:
                    plain_csr.append([r[0], r[1], r[2], r[3], r[4]])
            story.append(build_table(plain_csr, col_widths=[95, 60, 135, 135, 45]))
            story.append(Spacer(1, 4))

        sub_header("How to read them")
        load_prose = []
        if len(load_summary_rows) > 1:
            for row in load_summary_rows[1:]:
                load_prose.append(f"{row[0]} loaded {row[1]} in {row[2]} ({row[3]}).")
        if load_prose:
            story.append(Paragraph(" ".join(load_prose), body))
        story.append(Paragraph(
            "Row-Level Security isolates tenants on shared storage tables. "
            "Subscriptions and context source registrations are indexed in-memory per tenant.",
            body
        ))

    # SECTION 3: STARTUP AND IDLE FOOTPRINT
    startup_rows = tables.get("startup", [])
    if startup_rows and len(startup_rows) > 1:
        story.append(PageBreak())
        add_section_header("3. Cold Startup and Idle Footprint", "sec_startup")
        sub_header("What was measured")
        story.append(Paragraph(
            "Time elapsed from process execution until <code>GET /q/health</code> returns HTTP 200, "
            "and resident memory (VmRSS) occupied by the broker immediately after startup.",
            body
        ))
        sub_header("Under which conditions")
        story.append(Paragraph(
            "Cold binary startup across storage backends. Reported values represent the median of five cold boots.",
            body
        ))
        sub_header("The numbers")
        story.append(build_table(startup_rows, col_widths=[140, 165, 165]))

        sub_header("How to read them")
        startup_notes = []
        for r in startup_rows[1:]:
            st_name = r[0] if len(r) > 0 else ""
            ready_in = r[1] if len(r) > 1 else ""
            rss_val = r[2] if len(r) > 2 else ""
            if "memory" in st_name.lower():
                startup_notes.append(f"• <b>In-Memory Store:</b> Ready in {ready_in} with {rss_val} footprint. Suitable for ephemeral testing and edge deployments.")
            elif "file" in st_name.lower():
                startup_notes.append(f"• <b>Embedded File Store:</b> Ready in {ready_in} with {rss_val} footprint. Embedded persistence without external services.")
            elif "postgres" in st_name.lower():
                startup_notes.append(f"• <b>PostgreSQL Store:</b> Ready in {ready_in} with {rss_val} footprint including connection pool initialization. Production store.")
            else:
                startup_notes.append(f"• <b>{st_name}:</b> Ready in {ready_in} with {rss_val} footprint.")
        story.append(Paragraph("<br/>".join(startup_notes), body))

    # SECTION 4: READING THROUGHPUT
    if shapes_rows and len(shapes_rows) > 1:
        story.append(PageBreak())
        add_section_header("4. Reading Throughput: Who Called What", "sec_shapes")
        sub_header("What was measured")
        story.append(Paragraph(
            "Closed-loop throughput and tail latency under fixed client concurrency. "
            "Two query patterns: <b>query</b> (<code>GET /entities?type=Vehicle&limit=20</code>) "
            "and <b>retrieve</b> (<code>GET /entities/{id}</code>).",
            body
        ))
        sub_header("Under which conditions")
        story.append(Paragraph(
            "50 and 200 concurrent closed-loop clients (c50, c200). "
            "PostgreSQL rows execute against the loaded multi-tenant dataset. "
            "Memory rows run against a separate in-memory broker with 100 seeded entities.",
            body
        ))
        story.append(diagram_shapes())
        story.append(Paragraph("Figure 4.1: Closed-loop client interaction with query and retrieve routes.", caption))

        sub_header("The numbers")
        story.append(build_table(shapes_rows, col_widths=[90, 80, 80, 100, 120]))

        sub_header("How to read them")
        shapes_prose = []
        if q_rps_pg is not None and q_p99_pg is not None:
            shapes_prose.append(f"At 50 concurrent clients on PostgreSQL, query throughput reached {fmt_int(q_rps_pg)} req/s with p99 latency of {fmt_ms(q_p99_pg)}.")
        if ret_rps_pg is not None and ret_p99_pg is not None:
            shapes_prose.append(f"Single-entity retrieve reached {fmt_int(ret_rps_pg)} req/s at {fmt_ms(ret_p99_pg)} p99.")
        if q_rps_mem is not None:
            shapes_prose.append(f"In-memory store query throughput reached {fmt_int(q_rps_mem)} req/s at c50.")
        if q_rps_pg is not None:
            users_dash = 500
            req_per_user_sec = 0.2  # refresh every 5 seconds
            needed_rps = int(users_dash * req_per_user_sec)
            cap_pct = (needed_rps / q_rps_pg) * 100
            shapes_prose.append(f"A dashboard with {users_dash} users polling every 5 seconds requires {needed_rps} req/s, which is {cap_pct:.1f}% of the measured PostgreSQL capacity.")
        story.append(Paragraph(" ".join(shapes_prose), body))

    # SECTION 5: SATURATION KNEE
    saturate_rows = tables.get("saturate", [])
    if saturate_rows and len(saturate_rows) > 1:
        story.append(PageBreak())
        add_section_header("5. Saturation Knee", "sec_saturate")
        sub_header("What was measured")
        story.append(Paragraph(
            "Open-loop capacity limit. Arrival rate is stepped up in increments of 500 req/s until p99 latency "
            "exceeds 50 ms or error rate exceeds 0.1%. The saturation knee is the highest rate meeting both thresholds.",
            body
        ))
        sub_header("Under which conditions")
        story.append(Paragraph(
            "Open arrival rates enforced by k6 ramping-arrival-rate executor. "
            "'none reached' indicates that every tested stage satisfied both p99 and error thresholds.",
            body
        ))
        story.append(diagram_saturate())
        story.append(Paragraph("Figure 5.1: Stepped arrival rate ladder and threshold boundary.", caption))

        sub_header("The numbers")
        story.append(build_table(saturate_rows, col_widths=[75, 55, 95, 75, 95, 70, 75]))

        sub_header("How to read them")
        sat_prose = []
        for r in saturate_rows[1:]:
            st_name = r[0] if len(r) > 0 else ""
            sh_name = r[1] if len(r) > 1 else ""
            knee_rps = r[2] if len(r) > 2 else ""
            p99_knee = r[3] if len(r) > 3 else ""
            fail_stg = r[4] if len(r) > 4 else ""
            cores_val = r[5] if len(r) > 5 else ""
            sat_prose.append(f"For {st_name} {sh_name}, the knee was {knee_rps} rps ({p99_knee} p99, first failing stage: {fail_stg}, {cores_val} cores used).")
        story.append(Paragraph(" ".join(sat_prose), body))

    # SECTION 6: SUBSCRIPTIONS: NOTIFICATIONS UNDER UPDATE STREAM
    if fire_rows and len(fire_rows) > 1:
        story.append(PageBreak())
        add_section_header("6. Subscriptions: Notifications Under Update Stream", "sec_fire")
        sub_header("What was measured")
        story.append(Paragraph(
            "The notification delivery pipeline under write pressure. "
            "Entities are updated via <code>PATCH /attrs</code> and deleted via <code>DELETE</code>. "
            "The broker evaluates live subscriptions and delivers HTTP notification payloads to the receiver.",
            body
        ))
        sub_header("Under which conditions")
        story.append(Paragraph(
            f"{counted('subscriptions', 'live subscriptions')} spread over {counted('tenants', 'tenants')}, "
            f"in {len(subs_rows) - 1 if subs_rows else 'several'} filter classes (listed in section 2). "
            "One in five requests of the stream is a read of one entity, the rest are updates and deletes; "
            "the arrival rate is stepped up row by row.",
            body
        ))
        story.append(diagram_fire())
        story.append(Paragraph("Figure 6.1: End-to-end notification delivery pipeline stages.", caption))

        sub_header("The numbers")
        fire_summary = [["Rate (rps)", "Updates", "Failed", "Due", "Delivered", "Deliv %", "Dropped", "PATCH p99", "Broker Cores"]]
        for r in fire_rows[1:]:
            if len(r) >= 18:
                fire_summary.append([r[0], r[1], r[4], r[5], r[6], f"{r[7]}%", r[12], f"{r[14]} ms", r[16]])
            elif len(r) >= 8:
                fire_summary.append([r[0], r[1], r[4], r[5], r[6], f"{r[7]}%", "—", "—", "—"])
        story.append(build_table(fire_summary, col_widths=[55, 50, 50, 50, 55, 45, 50, 65, 50]))

        chart1, chart2 = chart_fire(out_path, fire_rows)
        if chart1:
            story.append(Spacer(1, 3))
            story.append(Image(chart1, width=CONTENT_WIDTH, height=135))
            story.append(Paragraph("Figure 6.2: Notification delivery percentage and dropped changes vs arrival rate.", caption))
        if chart2:
            story.append(Spacer(1, 3))
            story.append(Image(chart2, width=CONTENT_WIDTH, height=130))
            story.append(Paragraph("Figure 6.3: Response latency (p99) under increasing update arrival rate.", caption))

        # Class breakdown table
        fire_classes_rows = tables.get("fire-classes", [])
        if fire_classes_rows and len(fire_classes_rows) > 1:
            story.append(Paragraph("<b>Due and Delivered by Subscription Class</b>", h3))
            fc_table = [["Rate (rps)", "Class", "Due", "Delivered", "Delivered %"]]
            for r in fire_classes_rows[1:]:
                if len(r) >= 5:
                    fc_table.append([r[0], r[1], r[2], r[3], f"{r[4]}%"])
            story.append(build_table(fc_table, col_widths=[60, 130, 75, 75, 60]))
            story.append(Spacer(1, 4))

        sub_header("How to read them")
        fire_prose = []
        if safe_rate is not None:
            fire_prose.append(f"The highest arrival rate meeting >= 99% delivery with 0 failed operations was {safe_rate} updates/s.")
        if first_bad_rate is not None:
            fire_prose.append(f"At {first_bad_rate} updates/s, arrival exceeded drain capacity, resulting in {fmt_int(first_bad_drop)} dropped changes and PATCH p99 latency of {fmt_ms(first_bad_p99)}.")
        fire_prose.append("Dropped changes reflect a bounded in-memory change queue (1 024 entries) that sheds load when write arrival outpaces notification dispatch.")
        story.append(Paragraph(" ".join(fire_prose), body))

    # SECTION 7: CONTEXT SOURCE REGISTRATIONS AND FEDERATION
    if fed_rows and len(fed_rows) > 1:
        story.append(PageBreak())
        add_section_header("7. Context Source Registrations and Federated Queries", "sec_fed")
        sub_header("What was measured")
        story.append(Paragraph(
            "Query distribution via context source registrations. When an entity query arrives, "
            "the broker searches its <code>csource_index</code>, forwards partial queries to matching sources, "
            "merges responses, and returns the combined result.",
            body
        ))
        sub_header("Under which conditions")
        story.append(Paragraph(
            f"{counted('registrations', 'registrations')} spread over {counted('tenants', 'tenants')}. "
            "Five query shapes rotate over type, q, geoQ, scopeQ and idPattern filters. "
            "Remote endpoints resolve to the local sink answering with empty arrays.",
            body
        ))
        story.append(Paragraph(
            "A registration carries a <b>mode</b> (CIM 009 clauses 4.3.6.2 and 4.3.6.3). "
            "<b>inclusive</b>: the broker may hold the same data itself and still asks the source, merging both answers with equal priority. "
            "<b>auxiliary</b>: the source is asked for reads only and its data never overrides what the broker holds. "
            "<b>exclusive</b>: one source alone holds the named attributes of one entity; the broker keeps nothing about them locally. "
            "<b>redirect</b>: whole entities or types live at the source and every request for them is forwarded.",
            body
        ))
        story.append(diagram_fed())
        story.append(Paragraph("Figure 7.1: Query federation fan-out and result merging.", caption))

        sub_header("The numbers")
        story.append(build_table(fed_rows, col_widths=[50, 50, 70, 70, 60, 60, 55, 50, 50]))

        chart_f = chart_fed(out_path, fed_rows)
        if chart_f:
            story.append(Spacer(1, 3))
            story.append(Image(chart_f, width=CONTENT_WIDTH, height=130))
            story.append(Paragraph("Figure 7.2: Federated latency and source fan-out calls vs arrival rate.", caption))

        sub_header("How to read them")
        fed_prose = []
        calls_col = [parse_float(x) for x in col(fed_rows, "calls per query") if parse_float(x) is not None]
        mean_calls = sum(calls_col) / len(calls_col) if calls_col else None
        if mean_calls is not None:
            fed_prose.append(f"One query dialled {mean_calls:.1f} sources on average out of {counted('registrations', 'registrations')}: the registration index narrows the fan-out to the sources whose type, id pattern, location or scope match.")
        p99_fed = [parse_float(x) for x in col(fed_rows, "p99") if parse_float(x) is not None]
        rates_fed = [parse_int(x) for x in col(fed_rows, "rate") if parse_int(x) is not None]
        if p99_fed and len(p99_fed) >= 2:
            base_p99 = p99_fed[0]
            flat_rate = rates_fed[0]
            for r_val, p_val in zip(rates_fed, p99_fed):
                if p_val <= 2 * base_p99:
                    flat_rate = r_val
            fed_prose.append(f"The p99 stayed within twice its value at the lowest rate ({fmt_ms(base_p99)}) up to {flat_rate} queries/s.")
            above = [(r_val, p_val) for r_val, p_val in zip(rates_fed, p99_fed) if p_val > 2 * base_p99]
            if above:
                r_val, p_val = above[0]
                fed_prose.append(f"At {r_val} queries/s it reached {fmt_ms(p_val)}, {p_val / base_p99:.0f} times the flat value, so on this machine the federated path is safe below that rate.")
        story.append(Paragraph(" ".join(fed_prose), body))

    # SECTION 8: RESIDENT SET AND CPU UNDER LOAD
    story.append(PageBreak())
    add_section_header("8. Resident Set and CPU Under Load", "sec_rss")
    sub_header("What was measured")
    story.append(Paragraph(
        "Continuous 1 Hz sampling of physical memory (VmRSS) and CPU utilization across all rig components: "
        "Antares Broker, PostgreSQL, k6, HTTP Sink, and Mosquitto.",
        body
    ))
    sub_header("Under which conditions")
    story.append(Paragraph(
        f"Background sampling across execution window on {host_str}. 100% CPU represents one fully utilized core.",
        body
    ))

    cpu_png = out_path / "cpu.png"
    rss_csv = out_path / "rss.csv"
    if cpu_png.exists():
        story.append(Image(str(cpu_png), width=CONTENT_WIDTH, height=135))
        story.append(Paragraph("Figure 8.1: CPU core utilization over test execution phases.", caption))
    elif rss_csv.exists():
        gen_rss = chart_rss(out_path, rss_csv)
        if gen_rss:
            story.append(Image(gen_rss, width=CONTENT_WIDTH, height=135))
            story.append(Paragraph("Figure 8.1: Host and service core utilization.", caption))

    mem_png = out_path / "memory.png"
    if mem_png.exists():
        story.append(Spacer(1, 3))
        story.append(Image(str(mem_png), width=CONTENT_WIDTH, height=135))
        story.append(Paragraph("Figure 8.2: Resident Set Size (MiB) per service.", caption))

    sub_header("The numbers")
    if rss_rows:
        story.append(build_table(rss_rows, col_widths=[140, 165, 165]))

    sub_header("How to read them")
    rss_prose = []
    if b_rss is not None:
        rss_prose.append(f"Broker physical memory peaked at {b_rss}.")
    if p_rss is not None:
        rss_prose.append(f"PostgreSQL physical memory peaked at {p_rss}.")
    def peak_mean(cell: str, what: str, ceiling: str = "") -> str:
        parts = [x.strip() for x in cell.replace("cores", "").split("/")]
        if len(parts) == 2:
            return f"{what} used {parts[0]} cores at its peak{ceiling} and {parts[1]} on average."
        return f"{what}: {cell}{ceiling}."

    if b_cpu_peak is not None:
        rss_prose.append(peak_mean(b_cpu_peak, "The broker"))
    if h_cpu_peak is not None:
        cores_ceiling = f" of the {h_cores_max} available" if h_cores_max else ""
        rss_prose.append(peak_mean(h_cpu_peak, "The whole machine", cores_ceiling))
        rss_prose.append("A peak close to the core count means the machine, not the broker, was the limit in that phase; the rig's own load generator and receiver run on the same cores.")
    story.append(Paragraph(" ".join(rss_prose), body))

    # SECTION 9: APPENDIX AND MANIFEST
    story.append(PageBreak())
    add_section_header("9. Appendix and Manifest", "sec_appendix")

    manifest_path = out_path / "MANIFEST.md"
    if manifest_path.exists():
        sub_header("Artifact Manifest")
        m_lines = []
        for l in open(manifest_path).read().splitlines():
            if l.strip().startswith("-"):
                m_lines.append(l.strip())
            elif m_lines and l.startswith(" ") and l.strip():
                m_lines[-1] += " " + l.strip()
        manifest_items = []
        for l in m_lines:
            parts = l.lstrip("- ").split("—", 1)
            fname = html.escape(parts[0].strip())
            desc = html.escape(parts[1].strip()) if len(parts) > 1 else ""
            manifest_items.append([Paragraph(f"<code>{fname}</code>", body), Paragraph(desc, body)])
        if manifest_items:
            story.append(build_table(manifest_items, col_widths=[130, CONTENT_WIDTH - 130]))

    health_path = out_path / "health-final.json"
    if health_path.exists():
        story.append(Spacer(1, 6))
        sub_header("Final Health and Runtime Memory State")
        try:
            hdata = json.load(open(health_path))
            mem = hdata.get("memory", {})
            alloc_mb = mem.get("allocatedBytes", 0) / (1024 * 1024)
            res_mb = mem.get("residentBytes", 0) / (1024 * 1024)
            limits = hdata.get("limits", {})
            health_rows = [
                ["Jemalloc Allocated", f"{alloc_mb:.1f} MiB", "Change Queue Depth", str(limits.get("changeQueue", "—"))],
                ["Jemalloc Resident", f"{res_mb:.1f} MiB", "Max Batch Items", str(limits.get("maxBatchItems", "—"))],
                ["Changes Dropped", str(hdata.get("changesDropped", 0)), "Max Body Bytes", str(limits.get("maxBodyBytes", "—"))],
                ["Dead Letters", str(hdata.get("deadLetters", 0)), "Max URI Bytes", str(limits.get("maxUriBytes", "—"))],
            ]
            story.append(build_table(health_rows, col_widths=[120, 115, 120, 115]))
        except Exception:
            pass

    # Insert Table of Contents at the remembered marker index
    toc_elements = [Paragraph("<b>Table of Contents</b>", h2)]
    for title, anchor in emitted_sections:
        toc_elements.append(Paragraph(f'<a href="#{anchor}">{title}</a>', body))
    toc_elements.append(Spacer(1, 10))
    story[toc_marker_idx:toc_marker_idx] = toc_elements

    doc = SimpleDocTemplate(
        str(pdf_filename),
        pagesize=A4,
        leftMargin=MARGIN,
        rightMargin=MARGIN,
        topMargin=MARGIN,
        bottomMargin=MARGIN,
    )

    def page_cb(c, doc_obj):
        if hasattr(c, "commit_sha"):
            c.commit_sha = commit_sha

    doc.build(story, canvasmaker=NumberedCanvas, onFirstPage=page_cb, onLaterPages=page_cb)
    print(f"{pdf_filename}")
    return str(pdf_filename)


def main():
    target_dir = sys.argv[1] if len(sys.argv) > 1 else "results/perf"
    p = Path(target_dir)
    record = {}
    perf_json = p / "perf.json"
    if perf_json.exists():
        try:
            record = json.load(open(perf_json))
        except Exception:
            pass

    if not record.get("tables"):
        record.setdefault("tables", {})
        for md_file in p.glob("*.md"):
            stem = md_file.stem
            record["tables"][stem] = md_table(open(md_file).read())

    if not record.get("commit"):
        record["commit"] = "local"
    if not record.get("host"):
        record["host"] = "local rig"

    build(str(p), record)


if __name__ == "__main__":
    main()
