function statusLabel(status: "pending" | "approved" | "rejected"): string {
	switch (status) {
		case "pending":
			return "Waiting";
		case "approved":
			return "Approved";
		case "rejected":
			return "Rejected";
	}
}

const res = statusLabel("approved");
console.log(res);
