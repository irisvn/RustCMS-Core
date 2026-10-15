// Slots beside a record (`PanelSlot` with `Placement::RecordAside`).
import { useSlots } from "@/api/custom";
import { CustomContentView } from "@/components/custom";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export function RecordSlots({ resource, recordKey, record }: { resource: string; recordKey: string; record: unknown }) {
  const slots = useSlots(resource, recordKey);
  // A slot is extra: no data (or an error) shows nothing rather than a broken page.
  if (!slots.data || slots.data.data.length === 0) return null;
  return (
    <div className="flex min-w-0 flex-col gap-4">
      {slots.data.data.map((slot) => (
        <Card key={slot.name} className="min-w-0">
          {slot.label && (
            <CardHeader>
              <CardTitle>{slot.label}</CardTitle>
            </CardHeader>
          )}
          <CardContent className={slot.label ? undefined : "pt-5"}>
            <CustomContentView kind="slot" content={slot.content} record={record} reload={() => void slots.refetch()} />
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
