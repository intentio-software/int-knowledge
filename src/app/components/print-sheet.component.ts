import { ChangeDetectionStrategy, Component, Input } from "@angular/core";
import { CommonModule } from "@angular/common";
import { SafeHtml } from "@angular/platform-browser";

/**
 * A note laid out for paper.
 *
 * Hidden on screen and revealed only while printing, so the sidebar, titlebar
 * and status bar never reach the page. It carries its own type and spacing
 * rather than inheriting the reader's: a screen is backlit, scrollable and
 * dark half the time, and paper is none of those things.
 */
@Component({
  selector: "app-print-sheet",
  standalone: true,
  imports: [CommonModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="sheet">
      <header class="sheet-head">
        <h1>{{ title }}</h1>
        @if (subtitle) {
          <p class="subtitle">{{ subtitle }}</p>
        }
      </header>
      <article class="body" [innerHTML]="html"></article>
    </div>
  `,
  styles: [
    `
      :host {
        display: none;
      }

      @media print {
        :host {
          display: block;
        }
        .sheet {
          /* Black on white regardless of the app's theme: printing a dark
             theme wastes ink and reads badly, and nobody means it. */
          color: #000;
          background: #fff;
          font-family: "Iowan Old Style", "Palatino Linotype", Palatino, Georgia, serif;
          font-size: 11pt;
          line-height: 1.55;
        }
        .sheet-head {
          margin-bottom: 1.2em;
          padding-bottom: 0.5em;
          border-bottom: 0.5pt solid #999;
        }
        h1 {
          margin: 0;
          font-size: 20pt;
          font-weight: 600;
          line-height: 1.2;
        }
        .subtitle {
          margin: 0.35em 0 0;
          font-size: 9pt;
          color: #555;
        }

        .body :is(h1, h2, h3, h4, h5, h6) {
          /* A heading alone at the foot of a page is the classic printing
             defect; keep it with what it introduces. */
          break-after: avoid;
          page-break-after: avoid;
          margin: 1.1em 0 0.35em;
          line-height: 1.25;
        }
        .body h1 { font-size: 16pt; }
        .body h2 { font-size: 13.5pt; }
        .body h3 { font-size: 12pt; }
        .body :is(h4, h5, h6) { font-size: 11pt; }

        .body p,
        .body li {
          orphans: 2;
          widows: 2;
        }
        .body :is(pre, blockquote, table, figure, img) {
          break-inside: avoid;
          page-break-inside: avoid;
        }
        .body img {
          max-width: 100%;
          height: auto;
        }
        .body pre {
          padding: 0.6em 0.8em;
          border: 0.5pt solid #ccc;
          border-radius: 3pt;
          background: #f6f6f6;
          font-size: 9pt;
          white-space: pre-wrap;
          word-wrap: break-word;
        }
        .body code {
          font-family: "SF Mono", Menlo, Consolas, monospace;
          font-size: 9.2pt;
        }
        .body blockquote {
          margin: 0.8em 0;
          padding-left: 0.9em;
          border-left: 1.5pt solid #bbb;
          color: #333;
        }
        .body table {
          width: 100%;
          border-collapse: collapse;
          font-size: 9.5pt;
        }
        .body :is(th, td) {
          padding: 0.3em 0.5em;
          border: 0.5pt solid #bbb;
          text-align: left;
        }
        .body th {
          background: #f0f0f0;
        }
        /* Links keep their text but lose the colour — a blue that cannot be
           clicked is just a colour. */
        .body a {
          color: #000;
          text-decoration: none;
          border-bottom: 0.5pt solid #999;
        }
        /* An unresolved wikilink is a dead end on paper; do not dress it up. */
        .body a.unresolved {
          border-bottom: none;
          color: #666;
        }
        /* An inlined note, set off so it is clear whose words they are. */
        .body .transclusion {
          margin: 0.9em 0;
          padding-left: 0.9em;
          border-left: 1.5pt solid #ddd;
          break-inside: avoid;
          page-break-inside: avoid;
        }
        .body .transclusion-title {
          display: block;
          margin-bottom: 0.3em;
          font-size: 9.5pt;
          font-weight: 600;
          color: #444;
          border-bottom: none;
        }
        /* The ↪ says "this is embedded" to someone who could click it. On
           paper the content is simply there, so the arrow is just a mark. */
        .body .embed-marker {
          display: none;
        }
        .body :is(hr) {
          border: none;
          border-top: 0.5pt solid #bbb;
        }
      }
    `
  ]
})
export class PrintSheetComponent {
  @Input() title = "";
  /** Where it came from and when, so a printed page can be traced back. */
  @Input() subtitle = "";
  @Input() html: SafeHtml | null = null;
}
