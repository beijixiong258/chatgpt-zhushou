import { invoke } from "./desktop";

export type Session = {
  id: string;
  title: string;
  project: string;
  project_name: string;
  cwd: string;
  preview: string;
  updated: number;
};

export type Entry = {
  key: string;
  turn: string;
  kind: string;
  label: string;
  text: string;
  editable: boolean;
  removable: boolean;
  callGroup: string | null;
  defaultCleanup: boolean;
};

export type SessionDocument = {
  revision: string;
  entries: Entry[];
  notice?: string;
};

type Change = { key: string; text: string | null };

export function listSessions() {
  return invoke<Session[]>("history_list");
}

export function openSession(id: string) {
  return invoke<SessionDocument>("history_open", { id });
}

export function saveSession(id: string, revision: string, changes: Change[]) {
  return invoke<SessionDocument>("history_save", { id, revision, changes });
}

export function deleteSession(id: string, revision: string) {
  return invoke<void>("history_delete", { id, revision });
}
