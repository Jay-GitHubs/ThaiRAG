export function parsePrometheusMetric(text: string, name: string): number {
  const lines = text.split('\n');
  let total = 0;
  for (const line of lines) {
    if (line.startsWith(name)) {
      const parts = line.split(' ');
      const val = parseFloat(parts[parts.length - 1]);
      if (!isNaN(val)) total += val;
    }
  }
  return total;
}
