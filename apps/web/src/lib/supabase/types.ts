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
