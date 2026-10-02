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

const MODEL_COLOR_TOKENS: Record<string, string> = {
  "flight-deck-blue": "#2f9fd0",
  "flight-deck-teal": "#3aa7a3",
  "flight-deck-indigo": "#5e83d8",
  "flight-deck-green": "#2dbe8f",
  "flight-deck-violet": "#8f7be8",
  "flight-deck-amber": "#d19a4a",
  "flight-deck-rose": "#d46d7d",
  "flight-deck-sky": "#6db6e8"
};

export function colorForToken(token: string | undefined, fallbackModel: string): string {
  return token ? MODEL_COLOR_TOKENS[token] ?? colorForModel(fallbackModel) : colorForModel(fallbackModel);
}
