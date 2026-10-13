#include "setup.hpp"

void main_setup() {
	const uint D=DIAMETER;
	const uint scale=D/20u;
	const uint Nx=15u*D, Ny=8u*D, Nz=8u*D;
	const float u=0.04f, d=(float)D;
	LBM lbm(Nx, Ny, Nz, u*d/50.0f);
	const float cx=4.0f*d-0.5f, cy=0.5f*(float)Ny-0.5f, cz=0.5f*(float)Nz-0.5f;
	parallel_for(lbm.get_N(), [&](ulong n) { uint x=0u, y=0u, z=0u; lbm.coordinates(n, x, y, z);
		const float dx=(float)x-cx, dy=(float)y-cy, dz=(float)z-cz;
		if(dx*dx+dy*dy+dz*dz<=0.25f*d*d) lbm.flags[n] = TYPE_S;
		else lbm.u.x[n] = u;
		if(x==0u||x==Nx-1u) lbm.flags[n] = TYPE_E;
	});
	lbm.run(0u);
	const auto start = std::chrono::steady_clock::now();
	lbm.run(26000ull*scale);
	double sum=0.0; ulong samples=0ull;
	for(ulong t=0ull; t<10000ull*scale; t+=100ull) {
		lbm.run(100u);
		lbm.update_force_field();
		sum += (double)lbm.object_force(TYPE_S).x;
		samples++;
	}
	const double seconds = std::chrono::duration<double>(std::chrono::steady_clock::now()-start).count();
	print_info("SPHERE D "+to_string(D)+" cd "+to_string(sum/(double)samples/(0.5*u*u*3.14159265*d*d/4.0), 4u)+" seconds "+to_string(seconds, 1u));
}
