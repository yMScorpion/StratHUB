from symphony.workspace import sanitize_ref


def test_sanitize_ref_keeps_git_branch_safe_slug() -> None:
    assert sanitize_ref("ENG-123: Add Risk Limits!") == "eng-123-add-risk-limits"
    assert sanitize_ref("...") == "..."
    assert sanitize_ref("   ") == "issue"
