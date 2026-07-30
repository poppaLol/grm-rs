const MODEL_COLORS = [
  "#2f9fd0",
  "#3aa7a3",
  "#5e83d8",
  "#2dbe8f",
  "#8f7be8",
  "#d19a4a",
  "#d46d7d",
  "#6db6e8"
];

export function colorForModel(model: string): string {
  let hash = 0;
  for (const char of model) {
    hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  }
  return MODEL_COLORS[hash % MODEL_COLORS.length];
}
