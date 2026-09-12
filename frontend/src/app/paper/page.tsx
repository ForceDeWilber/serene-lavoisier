import { redirect } from "next/navigation";

export default function PaperRoute() {
  redirect("/dashboard?mode=paper");
}
