#include <array>
#include <random>
#include <string>

std::string randomAction() {
    static const std::array<std::string, 6> actions = {
        "睡觉", "奔跑", "喝水", "吃饭", "捕捉", "发呆"
    };
    static std::random_device rd;
    static std::mt19937 gen(rd());
    std::uniform_int_distribution<std::size_t> dist(0, actions.size() - 1);
    return actions[dist(gen)];
}
