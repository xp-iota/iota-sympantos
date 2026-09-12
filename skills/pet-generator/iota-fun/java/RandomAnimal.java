import java.util.concurrent.ThreadLocalRandom;

public class RandomAnimal {
    private static final String[] ANIMALS = {"猫", "狗", "鸟"};

    public static String randomAnimal() {
        int index = ThreadLocalRandom.current().nextInt(ANIMALS.length);
        return ANIMALS[index];
    }
}

