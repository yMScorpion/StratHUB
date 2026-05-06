// Minimal hand-written type stubs for the tables used in Phase 2.
// Replace with generated types (`supabase gen types typescript`) in Phase 3.

export type Json = string | number | boolean | null | { [key: string]: Json } | Json[];

export interface Database {
  public: {
    Tables: {
      strategy_jobs: {
        Row: {
          id: string;
          user_id: string;
          idempotency_key: string;
          status: "queued" | "processing" | "needs_review" | "approved" | "rejected" | "failed";
          pdf_count: number;
          arq_job_id: string | null;
          submitted_at: string | null;
          completed_at: string | null;
          error: string | null;
          created_at: string;
          updated_at: string;
        };
        Insert: Partial<Database["public"]["Tables"]["strategy_jobs"]["Row"]>;
        Update: Partial<Database["public"]["Tables"]["strategy_jobs"]["Row"]>;
      };
      strategies: {
        Row: {
          id: string;
          user_id: string;
          job_id: string | null;
          spec_jsonb: Json;
          spec_hash: string;
          status: "needs_review" | "approved" | "rejected";
          reviewed_by: string | null;
          reviewed_at: string | null;
          created_at: string;
          updated_at: string;
        };
        Insert: Partial<Database["public"]["Tables"]["strategies"]["Row"]>;
        Update: Partial<Database["public"]["Tables"]["strategies"]["Row"]>;
      };
      market_data_snapshots: {
        Row: {
          id: string;
          venue: string;
          symbol: string;
          timeframe: string;
          storage_uri: string;
          storage_format: "duckdb_parquet" | "json_fixture";
          row_count: number;
          starts_at: string | null;
          ends_at: string | null;
          fee_model_jsonb: Json;
          funding_jsonb: Json;
          spread_jsonb: Json;
          contract_jsonb: Json;
          lot_jsonb: Json;
          margin_jsonb: Json;
          liquidation_jsonb: Json;
          delistings_jsonb: Json;
          content_sha256: string;
          created_at: string;
        };
        Insert: Partial<Database["public"]["Tables"]["market_data_snapshots"]["Row"]>;
        Update: Partial<Database["public"]["Tables"]["market_data_snapshots"]["Row"]>;
      };
      backtests: {
        Row: {
          id: string;
          user_id: string;
          strategy_id: string;
          spec_hash: string;
          data_snapshot_id: string;
          status: "queued" | "running" | "succeeded" | "failed";
          mode: "backtest";
          initial_equity: number;
          kpi_jsonb: Json;
          equity_curve_jsonb: Json;
          heatmap_jsonb: Json;
          trades_jsonb: Json;
          error: string | null;
          started_at: string | null;
          completed_at: string | null;
          created_at: string;
          updated_at: string;
        };
        Insert: Partial<Database["public"]["Tables"]["backtests"]["Row"]>;
        Update: Partial<Database["public"]["Tables"]["backtests"]["Row"]>;
      };
      pdf_uploads: {
        Row: {
          id: string;
          user_id: string;
          job_id: string;
          filename: string;
          sha256: string;
          file_size_bytes: number;
          storage_path: string;
          page_count: number | null;
          ocr_confidence: number | null;
          status: "pending" | "uploaded" | "processing" | "done" | "error";
          error: string | null;
          created_at: string;
          updated_at: string;
        };
        Insert: Partial<Database["public"]["Tables"]["pdf_uploads"]["Row"]>;
        Update: Partial<Database["public"]["Tables"]["pdf_uploads"]["Row"]>;
      };
    };
    Views: Record<string, never>;
    Functions: Record<string, never>;
    Enums: Record<string, never>;
  };
}
