/**
 * lazypaw-js — Supabase-style client for lazypaw (PostgREST for SQL Server)
 */

// ─── Types ───────────────────────────────────────────────────────────

export interface LazypawClientOptions {
  /** JWT token for authentication */
  token?: string;
  /** Custom headers */
  headers?: Record<string, string>;
}

export interface QueryResult<T = any> {
  data: T[] | null;
  error: LazypawError | null;
  count?: number;
}

export interface SingleResult<T = any> {
  data: T | null;
  error: LazypawError | null;
}

export interface MutationResult<T = any> {
  data: T[] | null;
  error: LazypawError | null;
}

export interface LazypawError {
  message: string;
  code?: string;
  details?: string;
  hint?: string;
}

export type ChangeEvent<T = any> = {
  type: 'INSERT' | 'UPDATE' | 'DELETE';
  table: string;
  record: T;
};

export type ChangeCallback<T = any> = (payload: ChangeEvent<T>) => void;

// ─── Query Builder ───────────────────────────────────────────────────

class QueryBuilder<T = any> {
  private url: string;
  private defaultHeaders: Record<string, string>;
  private table: string;
  private selectStr?: string;
  private filters: string[] = [];
  private orderStr?: string;
  private limitVal?: number;
  private offsetVal?: number;
  private method: string = 'GET';
  private body?: any;
  private prefer: string[] = [];
  private isSingle: boolean = false;
  private isMaybeSingle: boolean = false;
  private wantCount: boolean = false;

  constructor(url: string, headers: Record<string, string>, table: string) {
    this.url = url;
    this.defaultHeaders = headers;
    this.table = table;
  }

  /** Select columns and embeds: .select('name, orders(id, amount)') */
  select(columns: string = '*'): this {
    this.selectStr = columns;
    return this;
  }

  // ─── Filters ─────────────────────────────

  eq(column: string, value: any): this {
    this.filters.push(`${column}=eq.${value}`);
    return this;
  }

  neq(column: string, value: any): this {
    this.filters.push(`${column}=neq.${value}`);
    return this;
  }

  gt(column: string, value: any): this {
    this.filters.push(`${column}=gt.${value}`);
    return this;
  }

  gte(column: string, value: any): this {
    this.filters.push(`${column}=gte.${value}`);
    return this;
  }

  lt(column: string, value: any): this {
    this.filters.push(`${column}=lt.${value}`);
    return this;
  }

  lte(column: string, value: any): this {
    this.filters.push(`${column}=lte.${value}`);
    return this;
  }

  like(column: string, pattern: string): this {
    this.filters.push(`${column}=like.${pattern}`);
    return this;
  }

  ilike(column: string, pattern: string): this {
    this.filters.push(`${column}=ilike.${pattern}`);
    return this;
  }

  is(column: string, value: 'null' | 'true' | 'false'): this {
    this.filters.push(`${column}=is.${value}`);
    return this;
  }

  in(column: string, values: any[]): this {
    this.filters.push(`${column}=in.(${values.join(',')})`);
    return this;
  }

  /** Full-text search: .textSearch('description', 'game') */
  textSearch(column: string, query: string): this {
    this.filters.push(`${column}=fts.${query}`);
    return this;
  }

  /** Negate a filter: .not('status', 'eq', 'inactive') */
  not(column: string, operator: string, value: any): this {
    this.filters.push(`${column}=not.${operator}.${value}`);
    return this;
  }

  /** Order results: .order('name', { ascending: true }) */
  order(column: string, opts?: { ascending?: boolean }): this {
    const dir = opts?.ascending === false ? 'desc' : 'asc';
    this.orderStr = `${column}.${dir}`;
    return this;
  }

  /** Limit number of rows */
  limit(count: number): this {
    this.limitVal = count;
    return this;
  }

  /** Offset for pagination */
  offset(count: number): this {
    this.offsetVal = count;
    return this;
  }

  /** Pagination range (inclusive): .range(0, 9) gets first 10 rows */
  range(from: number, to: number): this {
    this.offsetVal = from;
    this.limitVal = to - from + 1;
    return this;
  }

  /** Return a single row (first match). Sends Accept header for single object. */
  single(): this {
    this.isSingle = true;
    this.limitVal = 1;
    return this;
  }

  /** Like single() but returns null instead of error when 0 rows */
  maybeSingle(): this {
    this.isMaybeSingle = true;
    this.limitVal = 1;
    return this;
  }

  /** Request exact count via Content-Range header */
  count(): this {
    this.wantCount = true;
    return this;
  }

  // ─── Mutations ───────────────────────────

  /** Insert row(s) */
  insert(data: Partial<T> | Partial<T>[]): this {
    this.method = 'POST';
    this.body = data;
    this.prefer.push('return=representation');
    return this;
  }

  /** Upsert row(s) — insert with merge on conflict */
  upsert(data: Partial<T> | Partial<T>[]): this {
    this.method = 'POST';
    this.body = data;
    this.prefer.push('return=representation');
    this.prefer.push('resolution=merge-duplicates');
    return this;
  }

  /** Update matching rows */
  update(data: Partial<T>): this {
    this.method = 'PATCH';
    this.body = data;
    this.prefer.push('return=representation');
    return this;
  }

  /** Delete matching rows */
  delete(): this {
    this.method = 'DELETE';
    this.prefer.push('return=representation');
    return this;
  }

  // ─── Execute ─────────────────────────────

  private buildUrl(): string {
    const params: string[] = [];
    if (this.selectStr) params.push(`select=${encodeURIComponent(this.selectStr)}`);
    params.push(...this.filters);
    if (this.orderStr) params.push(`order=${this.orderStr}`);
    if (this.limitVal !== undefined) params.push(`limit=${this.limitVal}`);
    if (this.offsetVal !== undefined) params.push(`offset=${this.offsetVal}`);
    const qs = params.length ? '?' + params.join('&') : '';
    return `${this.url}/${this.table}${qs}`;
  }

  async then<TResult1 = QueryResult<T>, TResult2 = never>(
    resolve?: (value: QueryResult<T>) => TResult1 | PromiseLike<TResult1>,
    reject?: (reason: any) => TResult2 | PromiseLike<TResult2>,
  ): Promise<TResult1 | TResult2> {
    const result = await this.execute();
    if (resolve) return resolve(result);
    return result as any;
  }

  private async execute(): Promise<QueryResult<T>> {
    const url = this.buildUrl();
    const headers: Record<string, string> = {
      ...this.defaultHeaders,
      'Content-Type': 'application/json',
    };

    // Prefer headers
    const preferParts = [...this.prefer];
    if (this.wantCount) {
      preferParts.push('count=exact');
    }
    if (preferParts.length) {
      headers['Prefer'] = preferParts.join(', ');
    }

    // Accept header for single object
    if (this.isSingle) {
      headers['Accept'] = 'application/vnd.pgrst.object+json';
    }

    try {
      const res = await fetch(url, {
        method: this.method,
        headers,
        body: this.body ? JSON.stringify(this.body) : undefined,
      });

      if (!res.ok) {
        const err = await res.json().catch(() => ({ message: res.statusText }));
        return {
          data: null,
          error: {
            message: err.message || res.statusText,
            code: err.code,
            details: err.details,
            hint: err.hint,
          },
        };
      }

      // Parse count from Content-Range header if requested
      let count: number | undefined;
      if (this.wantCount) {
        const contentRange = res.headers.get('Content-Range');
        if (contentRange) {
          // Format: "0-9/100" or "*/100"
          const match = contentRange.match(/\/(\d+)/);
          if (match) {
            count = parseInt(match[1], 10);
          }
        }
      }

      const text = await res.text();
      if (!text) {
        const emptyData = (this.isSingle || this.isMaybeSingle) ? null : ([] as any);
        const result: QueryResult<T> = { data: emptyData, error: null };
        if (this.wantCount) result.count = count;
        return result;
      }

      const data = JSON.parse(text);

      if (this.isSingle) {
        // With Accept: application/vnd.pgrst.object+json, server returns object directly
        // Fallback: if it's still an array, unwrap it
        const row = Array.isArray(data) ? data[0] ?? null : data;
        const result: QueryResult<T> = { data: row, error: null } as any;
        if (this.wantCount) result.count = count;
        return result;
      }

      if (this.isMaybeSingle) {
        const row = Array.isArray(data) ? data[0] ?? null : data;
        const result: QueryResult<T> = { data: row, error: null } as any;
        if (this.wantCount) result.count = count;
        return result;
      }

      const result: QueryResult<T> = { data, error: null };
      if (this.wantCount) result.count = count;
      return result;
    } catch (e: any) {
      return { data: null, error: { message: e.message || 'Network error' } };
    }
  }
}

// ─── Realtime Channel ────────────────────────────────────────────────

class RealtimeChannel {
  private engine: RealtimeEngine;
  private table: string;
  private listeners: Array<{
    event: string;
    filter?: string;
    callback: ChangeCallback;
  }> = [];
  private subIds: string[] = [];

  constructor(engine: RealtimeEngine, table: string) {
    this.engine = engine;
    this.table = table;
  }

  /**
   * Listen for changes. Supports two signatures:
   *
   * Simple: .on('INSERT', callback)
   * Supabase-style: .on('postgres_changes', { event: 'INSERT', table: 'games', filter: 'id=eq.1' }, callback)
   */
  on(
    eventOrType: 'INSERT' | 'UPDATE' | 'DELETE' | '*' | 'postgres_changes',
    filterOrCb: string | Record<string, string> | ChangeCallback,
    maybeCb?: ChangeCallback,
  ): this {
    // Supabase-style: .on('postgres_changes', { event, table?, filter? }, callback)
    if (eventOrType === 'postgres_changes' && typeof filterOrCb === 'object' && typeof filterOrCb !== 'function') {
      const opts = filterOrCb as Record<string, string>;
      const event = (opts.event || '*') as string;
      const filter = opts.filter;
      // opts.table is informational (channel already scoped to a table)
      this.listeners.push({ event, filter, callback: maybeCb! });
      return this;
    }

    let filter: string | undefined;
    let callback: ChangeCallback;

    if (typeof filterOrCb === 'function') {
      callback = filterOrCb;
    } else if (typeof filterOrCb === 'object' && 'filter' in filterOrCb) {
      filter = (filterOrCb as Record<string, string>).filter;
      callback = maybeCb!;
    } else if (typeof filterOrCb === 'string') {
      filter = filterOrCb;
      callback = maybeCb!;
    } else {
      callback = maybeCb!;
    }

    this.listeners.push({ event: eventOrType, filter, callback });
    return this;
  }

  /** Subscribe to the channel (opens websocket subscriptions) */
  subscribe(): this {
    // Group by unique filter to minimize subscriptions
    const uniqueFilters = new Set(this.listeners.map((l) => l.filter || ''));
    const uniqueEvents = new Set(this.listeners.map((l) => l.event));
    const events =
      uniqueEvents.has('*')
        ? ['INSERT', 'UPDATE', 'DELETE']
        : [...uniqueEvents];

    for (const filter of uniqueFilters) {
      const subId = `${this.table}_${Math.random().toString(36).slice(2, 8)}`;
      this.subIds.push(subId);

      this.engine.subscribe(subId, this.table, filter || undefined, events, (event) => {
        for (const listener of this.listeners) {
          if (listener.event === '*' || listener.event === event.type) {
            if (!listener.filter || listener.filter === filter) {
              listener.callback(event);
            }
          }
        }
      });
    }
    return this;
  }

  /** Unsubscribe from all subscriptions in this channel */
  unsubscribe(): void {
    for (const subId of this.subIds) {
      this.engine.unsubscribe(subId);
    }
    this.subIds = [];
    this.listeners = [];
  }
}

// ─── Realtime Engine (WebSocket) ─────────────────────────────────────

class RealtimeEngine {
  private url: string;
  private token?: string;
  private ws: WebSocket | null = null;
  private callbacks: Map<string, ChangeCallback> = new Map();
  private pending: Array<() => void> = [];
  private connected: boolean = false;
  private reconnectTimer: any = null;
  private reconnectMs: number = 1000;

  constructor(url: string, token?: string) {
    // Convert http(s) to ws(s)
    this.url = url.replace(/^http/, 'ws') + '/realtime';
    this.token = token;
  }

  private connect(): void {
    if (this.ws) return;

    const wsUrl = this.token ? `${this.url}?token=${this.token}` : this.url;
    this.ws = new WebSocket(wsUrl);

    this.ws.onopen = () => {
      this.connected = true;
      this.reconnectMs = 1000;
      // Send any pending subscriptions
      for (const fn of this.pending) fn();
      this.pending = [];
    };

    this.ws.onmessage = (evt: MessageEvent) => {
      try {
        const msg = JSON.parse(evt.data);
        if (msg.type === 'change' || msg.type === 'INSERT' || msg.type === 'UPDATE' || msg.type === 'DELETE') {
          // Find callback by subscription id
          const cb = this.callbacks.get(msg.id);
          if (cb) {
            cb({
              type: msg.type === 'change' ? msg.record?.type : msg.type,
              table: msg.table,
              record: msg.record,
            });
          }
        }
      } catch {}
    };

    this.ws.onclose = () => {
      this.connected = false;
      this.ws = null;
      // Reconnect with backoff
      this.reconnectTimer = setTimeout(() => {
        this.reconnectMs = Math.min(this.reconnectMs * 2, 30000);
        this.connect();
      }, this.reconnectMs);
    };

    this.ws.onerror = () => {
      this.ws?.close();
    };
  }

  subscribe(
    subId: string,
    table: string,
    filter: string | undefined,
    events: string[],
    callback: ChangeCallback,
  ): void {
    this.callbacks.set(subId, callback);

    const send = () => {
      this.ws?.send(
        JSON.stringify({
          type: 'subscribe',
          id: subId,
          table,
          filter: filter || undefined,
          events,
        }),
      );
    };

    if (!this.ws) this.connect();
    if (this.connected) {
      send();
    } else {
      this.pending.push(send);
    }
  }

  unsubscribe(subId: string): void {
    this.callbacks.delete(subId);
    if (this.connected && this.ws) {
      this.ws.send(JSON.stringify({ type: 'unsubscribe', id: subId }));
    }
  }

  disconnect(): void {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.ws?.close();
    this.ws = null;
    this.connected = false;
    this.callbacks.clear();
  }
}

// ─── Client ──────────────────────────────────────────────────────────

export class LazypawClient {
  private url: string;
  private headers: Record<string, string>;
  private realtimeEngine: RealtimeEngine;

  constructor(url: string, options?: LazypawClientOptions) {
    // Remove trailing slash
    this.url = url.replace(/\/$/, '');
    this.headers = { ...options?.headers };
    if (options?.token) {
      this.headers['Authorization'] = `Bearer ${options.token}`;
    }
    this.realtimeEngine = new RealtimeEngine(this.url, options?.token);
  }

  /** Start a query on a table: lp.from('users').select('*').eq('active', true) */
  from<T = any>(table: string): QueryBuilder<T> {
    return new QueryBuilder<T>(this.url, this.headers, table);
  }

  /** Call a stored procedure/function: lp.rpc('get_leaderboard', { game_id: 1 }) */
  async rpc<T = any>(fn: string, args?: Record<string, any>): Promise<QueryResult<T>> {
    const url = `${this.url}/rpc/${fn}`;
    const headers: Record<string, string> = {
      ...this.headers,
      'Content-Type': 'application/json',
    };

    try {
      const res = await fetch(url, {
        method: 'POST',
        headers,
        body: args ? JSON.stringify(args) : undefined,
      });

      if (!res.ok) {
        const err = await res.json().catch(() => ({ message: res.statusText }));
        return {
          data: null,
          error: {
            message: err.message || res.statusText,
            code: err.code,
            details: err.details,
            hint: err.hint,
          },
        };
      }

      const text = await res.text();
      if (!text) return { data: null, error: null };

      const data = JSON.parse(text);
      return { data, error: null };
    } catch (e: any) {
      return { data: null, error: { message: e.message || 'Network error' } };
    }
  }

  /** Create a realtime channel for a table */
  channel(table: string): RealtimeChannel {
    return new RealtimeChannel(this.realtimeEngine, table);
  }

  /** Disconnect realtime */
  disconnect(): void {
    this.realtimeEngine.disconnect();
  }
}

// ─── Factory ─────────────────────────────────────────────────────────

/** Create a lazypaw client */
export function createClient(url: string, options?: LazypawClientOptions): LazypawClient {
  return new LazypawClient(url, options);
}

export default createClient;
