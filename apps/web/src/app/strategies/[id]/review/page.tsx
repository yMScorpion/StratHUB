// Human review gate — Phase 3.

export default async function ReviewPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  return (
    <main style={{ padding: "4rem" }}>
      <h1 style={{ fontWeight: 600, fontSize: "1.5rem" }}>Strategy review</h1>
      <p style={{ opacity: 0.6, marginTop: "0.5rem" }}>
        Strategy <code>{id}</code> — full review UI ships in Phase 3.
      </p>
    </main>
  );
}
