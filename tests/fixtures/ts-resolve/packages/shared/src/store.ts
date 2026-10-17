export interface Store {
  get(id: string): string;
}

export class MemoryStore implements Store {
  get(id: string): string {
    return id.trim();
  }
}
