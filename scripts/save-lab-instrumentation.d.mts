export function saveLabInstrumentation(): {
  name: string;
  enforce: 'pre';
  transform(code: string, id: string): string | undefined;
};
