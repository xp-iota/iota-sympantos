const COLORS = ["red", "blue", "green", "yellow", "black", "white"];

export function randomColor(): string {
  const index = Math.floor(Math.random() * COLORS.length);
  return COLORS[index];
}

