// Parse common LaTeX delimiters while preserving literal inline/fenced code.
// The caller supplies the placeholder token: it must be unique per render so
// that text the model typed can never be mistaken for one of our own markers.
export function splitMath(text: string, token = 'MATHPLACEHOLDER') {
  const formulas: { value: string; display: boolean }[] = [];
  const source = text.slice(0, 65536).replace(/```[^]*?```|`[^`\n]*`|\$\$([^]*?)\$\$|\$([^$\n]+)\$|\\\[([^]*?)\\\]|\\\(([^]*?)\\\)/g,
    (all, dollars, inline, brackets, parens) => {
      if (all.startsWith('`')) return all;
      const id = formulas.push({ value: dollars ?? brackets ?? inline ?? parens, display: dollars != null || brackets != null }) - 1;
      return `${token}${id}END`;
    });
  return { source, formulas };
}
