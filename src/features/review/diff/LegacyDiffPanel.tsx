import type { ReviewState } from "../../../lib/store";

interface LegacyDiffPanelProps {
  state: ReviewState;
}

export function LegacyDiffPanel({ state }: LegacyDiffPanelProps) {
  const diffLines = state.diffText!.split("\n");
  let displayLines: string[];

  if (state.selectedFile) {
    displayLines = extractFileDiff(diffLines, state.selectedFile);
  } else {
    displayLines = diffLines;
  }

  return (
    <div className="overflow-auto h-full">
      <pre className="text-xs font-mono p-4 leading-5">
        {displayLines.map((line, i) => (
          <div
            key={i}
            className={
              line.startsWith("+") && !line.startsWith("+++")
                ? "bg-emerald-900/20 text-emerald-300"
                : line.startsWith("-") && !line.startsWith("---")
                  ? "bg-red-900/20 text-red-300"
                  : line.startsWith("@@")
                    ? "text-blue-400"
                    : line.startsWith("diff ")
                      ? "text-zinc-400 font-bold mt-2"
                      : "text-zinc-400"
            }
          >
            {line}
          </div>
        ))}
      </pre>
    </div>
  );
}

function extractFileDiff(lines: string[], filePath: string): string[] {
  const result: string[] = [];
  let inFile = false;

  for (const line of lines) {
    if (line.startsWith("diff --git")) {
      inFile = line.includes(`b/${filePath}`) || line.includes(filePath);
    }
    if (inFile) {
      result.push(line);
    }
  }

  return result.length > 0 ? result : [`No diff found for ${filePath}`];
}
