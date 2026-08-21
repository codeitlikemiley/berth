import { useParams } from "react-router-dom";

export function LeasePage() {
  const { id } = useParams();
  return (
    <main className="p-6">
      <h1 className="text-xl font-medium">Lease</h1>
      <p className="text-muted-foreground">{id}</p>
    </main>
  );
}
