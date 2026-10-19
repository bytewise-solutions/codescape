import express from 'express';

export type Reply = { ok: boolean };

export type Http = {
  <T>(url: string): Reply;
  get: <T>(url: string) => Reply;
  post(url: string, body?: string): Reply;
  create: (base: string) => Http;
};

export type Server = { port: number; close: () => void } & express.Express;

export type Mixed = Http & { extra(): number };

export const http: Http = makeHttp();

function makeHttp(): Http {
  return null as unknown as Http;
}
