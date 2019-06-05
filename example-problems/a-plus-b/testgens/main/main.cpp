//magicbuild:link=jtl
#include <cstdlib>
#include <cstdio>
#include <jtl.h>

int main(int argc, char** argv) {
    int test_id = get_env_int("JJS_TEST_ID");
    int test_out_fd = get_env_int("JJS_TEST");
    FILE* test = fdopen(test_out_fd, "w");
    fprintf(test, "%d %d\n", test_id, test_id * 2 + 1);
}