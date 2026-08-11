import re
import unittest

CI_PATH = ".github/workflows/ci.yml"
TERMUX_PATH = ".github/workflows/termux.yml"
RELEASE_PATH = ".github/workflows/release.yml"


class TestTermuxWorkflowContract(unittest.TestCase):
    def read_file(self, path):
        with open(path, "r", encoding="utf-8") as f:
            return f.read()

    @staticmethod
    def _indented_block(content, header):
        """Return YAML-free block beginning at header, preserving structure."""
        lines = content.splitlines()
        for index, line in enumerate(lines):
            if line.strip() != header:
                continue
            indent = len(line) - len(line.lstrip())
            block = [line]
            for candidate in lines[index + 1 :]:
                if candidate.strip() and len(candidate) - len(candidate.lstrip()) <= indent:
                    break
                block.append(candidate)
            return "\n".join(block)
        raise AssertionError(f"missing workflow block: {header}")

    def assert_termux_contract(self, termux, ci):
        dispatch = self._indented_block(termux, "workflow_dispatch:")
        call = self._indented_block(termux, "workflow_call:")
        for block in (dispatch, call):
            self.assertRegex(block, r"(?m)^\s{6}source_sha:\s*$")
            self.assertRegex(block, r"(?m)^\s{8}required:\s+true\s*$")
            self.assertRegex(block, r"(?m)^\s{8}type:\s+string\s*$")

        permissions = self._indented_block(termux, "permissions:")
        self.assertRegex(permissions, r"(?m)^\s{2}contents:\s+read\s*$")
        self.assertNotRegex(permissions, r"(?m)^\s{2}contents:\s+write\s*$")

        config = self._indented_block(termux, "runner-config:")
        self.assertRegex(config, r"(?m)^\s{6}source_sha:\s+\$\{\{\s*steps\.validate\.outputs\.source_sha\s*\}\}")
        self.assertIn("case \"$INPUT_SHA\"", config)
        self.assertIn("source_sha=$INPUT_SHA\" >> \"$GITHUB_OUTPUT\"", config)

        job = self._indented_block(termux, "termux:")
        self.assertIn("needs: runner-config", job)
        self.assertRegex(job, r"(?m)^\s{4}environment:\s+termux-attestation\s*$")
        self.assertRegex(job, r"(?m)^\s{4}runs-on:\s+\$\{\{\s*fromJSON\(needs\.runner-config\.outputs\.labels\)\s*\}\}")
        self.assertIn("ref: ${{ needs.runner-config.outputs.source_sha }}", job)
        self.assertNotIn("ref: ${{ github.sha }}", job)
        self.assertIn('EXPECTED="${{ needs.runner-config.outputs.source_sha }}"', job)
        self.assertIn('test "$ACTUAL" = "$EXPECTED"', job)
        self.assertNotRegex(job, r"(?m)^\s*contents:\s*write\s*$")

        required = self._indented_block(ci, "ci-required:")
        self.assertRegex(required, r"(?m)^\s{6}-\s+termux-attestation\s*$")
        gate = self._indented_block(ci, "termux-attestation:")
        self.assertRegex(
            gate,
            r"github\.event\.pull_request\.head\.repo\.full_name\s*==\s*github\.repository",
        )
        self.assertNotRegex(gate, r"(?m)^\s*if:\s*(true|\$\{\{\s*true\s*\}\})\s*$")

    def test_termux_workflow_contract(self):
        termux = self.read_file(TERMUX_PATH)
        ci = self.read_file(CI_PATH)
        self.assertIn("timeout-minutes: 60", termux)
        self.assertIn("persist-credentials: false", termux)
        self.assertIn("sh ./scripts/verify-termux", termux)
        self.assertIn("if: always()", termux)
        self.assertNotIn("secrets: inherit", termux)
        self.assert_termux_contract(termux, ci)

    def test_mutation_source_sha_not_required_fails(self):
        termux = self.read_file(TERMUX_PATH).replace("required: true", "required: false", 1)
        with self.assertRaises(AssertionError):
            self.assert_termux_contract(termux, self.read_file(CI_PATH))

    def test_mutation_checkout_github_sha_fails(self):
        termux = self.read_file(TERMUX_PATH).replace(
            "ref: ${{ needs.runner-config.outputs.source_sha }}",
            "ref: ${{ github.sha }}",
        )
        with self.assertRaises(AssertionError):
            self.assert_termux_contract(termux, self.read_file(CI_PATH))

    def test_mutation_missing_sha_verification_fails(self):
        termux = self.read_file(TERMUX_PATH).replace('test "$ACTUAL" = "$EXPECTED"', "true")
        with self.assertRaises(AssertionError):
            self.assert_termux_contract(termux, self.read_file(CI_PATH))

    def test_mutation_missing_sha_output_fails(self):
        termux = self.read_file(TERMUX_PATH).replace(
            'echo "source_sha=$INPUT_SHA" >> "$GITHUB_OUTPUT"', "echo labels=$LABELS_RAW >> \"$GITHUB_OUTPUT\""
        )
        with self.assertRaises(AssertionError):
            self.assert_termux_contract(termux, self.read_file(CI_PATH))

    def test_mutation_termux_needs_removed_fails(self):
        ci = self.read_file(CI_PATH).replace("      - termux-attestation\n", "")
        with self.assertRaises(AssertionError):
            self.assert_termux_contract(self.read_file(TERMUX_PATH), ci)

    def test_mutation_contents_write_fails(self):
        termux = self.read_file(TERMUX_PATH).replace("  contents: read", "  contents: write")
        with self.assertRaises(AssertionError):
            self.assert_termux_contract(termux, self.read_file(CI_PATH))

    def test_mutation_fork_guard_true_fails(self):
        ci = self.read_file(CI_PATH).replace(
            "    if: >-\n      github.event_name != 'pull_request' ||\n      github.event.pull_request.head.repo.full_name == github.repository",
            "    if: true",
        )
        with self.assertRaises(AssertionError):
            self.assert_termux_contract(self.read_file(TERMUX_PATH), ci)

    def test_termux_dispatch_inputs_contract(self):
        content = self.read_file(TERMUX_PATH)
        self.assertIn("workflow_dispatch:", content)
        self.assertIn("inputs:", content)
        self.assertIn("source_sha:", content)
        self.assertIn("required: true", content)
        self.assertIn("type: string", content)

    def test_ci_workflow_contract(self):
        content = self.read_file(CI_PATH)
        self.assertIn("uses: ./.github/workflows/termux.yml", content)
        self.assertIn("source_sha: ${{ github.sha }}", content)
        self.assertIn("termux-attestation", content)
        self.assertIn("- termux-attestation", content)
        self.assertIn("--exclude-case termux.env", content)
        self.assertIn("github.event.pull_request.head.repo.full_name == github.repository", content)

    def test_release_workflow_contract(self):
        content = self.read_file(RELEASE_PATH)
        self.assertIn("uses: ./.github/workflows/ci.yml", content)
        self.assertIn("needs: [validate, ci]", content)
        self.assertNotIn("uses: ./.github/workflows/termux.yml", content)

    def test_no_pull_request_target(self):
        for path in (CI_PATH, TERMUX_PATH, RELEASE_PATH):
            self.assertNotIn("pull_request_target", self.read_file(path))


if __name__ == "__main__":
    unittest.main()
