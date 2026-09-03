import type { ReactNode } from "react";

// Deliberately small, inert Markdown subset. React escapes raw HTML/code;
// images, external URLs and unknown protocols never become network requests.
function inline(
  text: string,
  navigate: (href: string) => void,
  resolve: (href: string) => boolean,
): ReactNode[] {
  return text
    .split(/(`[^`]+`|\*\*[^*]+\*\*|!?\[[^\]]+\]\([^)]*\))/g)
    .map((part, index) => {
      if (part.startsWith("`") && part.endsWith("`"))
        return <code key={index}>{part.slice(1, -1)}</code>;
      if (part.startsWith("**") && part.endsWith("**"))
        return <strong key={index}>{part.slice(2, -2)}</strong>;
      const link = /^\[([^\]]+)\]\(([^)]*)\)$/.exec(part);
      if (link && resolve(link[2]!))
        return (
          <button
            className="help-inline-link"
            type="button"
            key={index}
            onClick={() => navigate(link[2]!)}
          >
            {link[1]}
          </button>
        );
      if (link)
        return (
          <span key={index}>
            {link[1]}（{link[2]}）
          </span>
        );
      return part;
    });
}

export function Markdown({
  body,
  navigate,
  resolve,
}: {
  body: string;
  navigate: (href: string) => void;
  resolve: (href: string) => boolean;
}) {
  const lines = body.split("\n");
  const blocks: ReactNode[] = [];
  const render = (value: string) => inline(value, navigate, resolve);
  let i = 0;
  while (i < lines.length) {
    const key = i;
    const line = lines[i++]!;
    if (!line.trim()) continue;
    if (line.startsWith("```")) {
      const code: string[] = [];
      while (i < lines.length && !lines[i]!.startsWith("```"))
        code.push(lines[i++]!);
      if (i < lines.length) i++;
      blocks.push(
        <pre key={key}>
          <code>{code.join("\n")}</code>
        </pre>,
      );
    } else if (/^#{3,6} /.test(line)) {
      blocks.push(<h3 key={key}>{render(line.replace(/^#+ /, ""))}</h3>);
    } else if (line.startsWith("|")) {
      const rows = [line];
      while (i < lines.length && lines[i]!.startsWith("|"))
        rows.push(lines[i++]!);
      const cells = (row: string) =>
        row
          .replace(/^\||\|$/g, "")
          .split("|")
          .map((cell) => cell.trim());
      const data = rows.filter((row) => !/^\|[\s:|-]+\|$/.test(row));
      blocks.push(
        <div className="help-table" key={key}>
          <table>
            <thead>
              <tr>
                {cells(data[0]!).map((cell, c) => (
                  <th key={c}>{render(cell)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {data.slice(1).map((row, r) => (
                <tr key={r}>
                  {cells(row).map((cell, c) => (
                    <td key={c}>{render(cell)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
    } else if (/^(?:- |\d+\. )/.test(line)) {
      const ordered = /^\d/.test(line);
      const pattern = ordered ? /^\d+\. / : /^- /;
      const items = [line.replace(pattern, "")];
      while (i < lines.length && pattern.test(lines[i]!))
        items.push(lines[i++]!.replace(pattern, ""));
      const children = items.map((item, j) => <li key={j}>{render(item)}</li>);
      blocks.push(
        ordered ? <ol key={key}>{children}</ol> : <ul key={key}>{children}</ul>,
      );
    } else if (line.startsWith("> ")) {
      blocks.push(<blockquote key={key}>{render(line.slice(2))}</blockquote>);
    } else {
      const paragraph = [line];
      while (
        i < lines.length &&
        lines[i]!.trim() &&
        !/^(?:#|```|\||- |> |\d+\. )/.test(lines[i]!)
      )
        paragraph.push(lines[i++]!);
      blocks.push(<p key={key}>{render(paragraph.join("\n"))}</p>);
    }
  }
  return <>{blocks}</>;
}
