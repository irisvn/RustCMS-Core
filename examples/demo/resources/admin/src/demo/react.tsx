// The React demo page (`ReactPage`, `/admin/p/react-demo`): host API v2, on
// the panel's React, React Query and UI kit. Built by `npm run build`.
import { useState } from "react";
import { type PageProps, registerPage, useAdminMutation, useAdminQuery, useEvents } from "@rustcms/admin-sdk";
import { Button, Card, CardContent, Dialog } from "@rustcms/admin-sdk/ui";
import "./react.css";

type Data = { path: string; items: Array<{ id: number; name: string }> };

function ReactDemo(props: PageProps<Data>) {
  const hello = useAdminQuery<{ message: string }>(props, "/demo/hello");
  const reset = useAdminMutation(props, "/demo/ticks/reset", "post");
  const [count, setCount] = useState<number | null>(null);
  const [open, setOpen] = useState(false);
  useEvents(props, "/demo/ticks", {
    onEvent(name, data) {
      if (name === "tick") setCount((data as { count: number }).count);
    },
  });
  const item = props.data.items.find((i) => props.path === `items/${i.id}`);
  return (
    <Card className="demo-react">
      <CardContent className="demo-react-body">
        <p>{hello.data?.message ?? "…"}</p>
        <p>
          {props.t("Count")}: <output aria-label="React count">{count ?? "–"}</output>
        </p>
        <Button onClick={() => reset.mutate({})}>{props.t("Reset counter")}</Button>
        <ul className="demo-react-items">
          {props.data.items.map((i) => (
            <li key={i.id}>
              <Button variant="link" onClick={() => props.navigate(`/p/react-demo/items/${i.id}`)}>
                {i.name}
              </Button>
            </li>
          ))}
        </ul>
        {item && <h2 className="demo-react-item">{`Item ${item.name}`}</h2>}
        <Button variant="outline" onClick={() => setOpen(true)}>
          {props.t("Open dialog")}
        </Button>
        <Dialog open={open} onClose={() => setOpen(false)} title={props.t("Demo dialog")}>
          <p className="demo-react-dialog">{props.t("Styled by the extension")}</p>
        </Dialog>
        {props.can("demo.live") && <p className="demo-react-can">{props.t("You may watch the live counter.")}</p>}
      </CardContent>
    </Card>
  );
}

registerPage("demo/react", ReactDemo);
