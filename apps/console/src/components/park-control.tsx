import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

export function ParkControl({
  parked,
  hasLive,
  pending,
  onPark,
  onUnpark,
}: {
  parked: boolean;
  hasLive: boolean;
  pending: boolean;
  onPark: () => void;
  onUnpark: () => void;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Park</CardTitle>
        <CardDescription>{parked ? "Parked" : "Unparked"}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant={parked ? "secondary" : "default"}
            disabled={pending}
            onClick={onPark}
          >
            Park
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={pending || hasLive}
            onClick={onUnpark}
          >
            Unpark
          </Button>
        </div>
        {hasLive ? (
          <p className="text-sm text-muted-foreground">
            end or force-disconnect the live session first.
          </p>
        ) : null}
      </CardContent>
    </Card>
  );
}
