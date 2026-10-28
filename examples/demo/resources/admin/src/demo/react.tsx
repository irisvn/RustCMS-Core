// The React demo page (`ReactPage`, `/admin/p/react-demo`): host API v2, on
// the panel's React, React Query and UI kit (level 2). Built by `npm run build`.
import { useEffect, useState } from "react";
import {
  type PageProps,
  fieldErrors,
  registerPage,
  useAdminMutation,
  useColorScheme,
  useAdminQuery,
  useEvents,
  useResource,
  useResourceMutation,
} from "@rustcms/admin-sdk";
import {
  Button,
  Card,
  CardContent,
  Dialog,
  DropdownMenu,
  DropdownMenuItem,
  DropdownMenuSeparator,
  Input,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Tooltip,
} from "@rustcms/admin-sdk/ui";
import "./react.css";

type Data = { path: string; items: Array<{ id: number; name: string }> };

type Tag = { name: string };

// #region tags
/** Tags through the resource API: the panel's policy, validation and audit apply. */
function Tags(props: PageProps<Data>) {
  const tags = useResource<Tag>(props, "tags", { query: { per_page: "100" } });
  const create = useResourceMutation(props, "tags", "create", {
    onSuccess: () => {
      setName("");
      props.toast.success(props.t("Tag created"));
    },
  });
  const remove = useResourceMutation(props, "tags", "delete", {
    onSuccess: () => props.toast.success(props.t("Tag deleted")),
  });
  const [name, setName] = useState("");
  const errors = fieldErrors(create.error);
  return (
    <div className="demo-react-tags">
      <form
        className="demo-react-row"
        onSubmit={(e) => {
          e.preventDefault();
          // As the panel's forms: an empty text is no value (`required` refuses it).
          create.mutate({ data: { name: name.trim() === "" ? null : name } });
        }}
      >
        <Input aria-label={props.t("Tag name")} value={name} onChange={(e) => setName(e.target.value)} />
        <Button type="submit">{props.t("Add tag")}</Button>
        <Tooltip content={props.t("Read the tags again")}>
          <Button type="button" variant="ghost" onClick={() => void tags.refetch()}>
            {props.t("Refresh")}
          </Button>
        </Tooltip>
      </form>
      {errors.name && <p role="alert" className="demo-react-error">{errors.name.join(" ")}</p>}
      <ul className="demo-react-list">
        {tags.data?.data.map((tag) => (
          <li key={tag.key} className="demo-react-row">
            <span>{tag.data.name}</span>
            <DropdownMenu trigger={props.t("Actions")} aria-label={`${props.t("Actions")} ${tag.data.name}`} variant="ghost">
              <DropdownMenuItem onSelect={() => props.navigate(`/r/tags/records/${tag.key}`)}>{props.t("Open")}</DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem destructive onSelect={() => remove.mutate({ key: tag.key })}>
                {props.t("Delete")}
              </DropdownMenuItem>
            </DropdownMenu>
          </li>
        ))}
      </ul>
    </div>
  );
}
// #endregion tags

function ReactDemo(props: PageProps<Data>) {
  const tab = props.path === "tags" ? "tags" : "overview";
  // #region title
  // The sub-path names itself in the panel's heading and breadcrumb.
  useEffect(() => {
    if (props.path === "tags") props.title(props.t("React demo: tags"));
  }, [props.path, props.title, props.t]);
  // #endregion title
  return (
    <Tabs value={tab} onValueChange={(v) => props.navigate(v === "tags" ? "/p/react-demo/tags" : "/p/react-demo")}>
      <TabsList aria-label={props.t("Sections")}>
        <TabsTrigger value="overview">{props.t("Overview")}</TabsTrigger>
        <TabsTrigger value="tags">{props.t("Tags")}</TabsTrigger>
      </TabsList>
      <TabsContent value="overview">
        <Overview {...props} />
      </TabsContent>
      <TabsContent value="tags">
        <Tags {...props} />
      </TabsContent>
    </Tabs>
  );
}

function Overview(props: PageProps<Data>) {
  const scheme = useColorScheme();
  const hello = useAdminQuery<{ message: string }>(props, "/demo/hello");
  const reset = useAdminMutation(props, "/demo/ticks/reset", "post");
  const [count, setCount] = useState<number | null>(null);
  const [open, setOpen] = useState(false);
  // #region use-events
  useEvents(props, "/demo/ticks", {
    onEvent(name, data) {
      if (name === "tick") setCount((data as { count: number }).count);
    },
  });
  // #endregion use-events
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
        <p className="demo-react-can">
          {props.t("Color scheme")}: <span aria-label="React scheme">{scheme}</span>
        </p>
        {props.can("demo.live") && <p className="demo-react-can">{props.t("You may watch the live counter.")}</p>}
      </CardContent>
    </Card>
  );
}

registerPage("demo/react", ReactDemo);
