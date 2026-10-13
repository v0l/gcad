#include "setup.hpp"

void main_setup() {
	const uint Nx=NX, Ny=NY, Nz=NZ;
	const float h=CELL, u=0.04f, d=25.0f/h, length=500.0f/h, across=111.5848f/h;
	const ulong settle=SETTLE, average=AVERAGE;
	LBM lbm(Nx, Ny, Nz, u*d/54900.0f);
	const float cx=0.5f*(float)Nx-0.5f, cy=0.5f*(float)Ny-0.5f, cz=CZ;
	parallel_for(lbm.get_N(), [&](ulong n) { uint x=0u, y=0u, z=0u; lbm.coordinates(n, x, y, z);
		const float dx=(float)x-cx, dz=(float)z-cz;
		if(dx*dx+dz*dz<=0.25f*d*d&&fabs((float)y-cy)<=0.5f*length) lbm.flags[n] = TYPE_S;
		else lbm.u.z[n] = u;
		if(x==0u||x==Nx-1u||y==0u||y==Ny-1u||z==0u||z==Nz-1u) lbm.flags[n] = TYPE_E;
	});
	lbm.run(0u);
	const auto start = std::chrono::steady_clock::now();
	lbm.run(settle);
	double sum=0.0; ulong samples=0ull;
	for(ulong t=0ull; t<average; t+=50ull) {
		lbm.run(50u);
		lbm.update_force_field();
		sum += (double)lbm.object_force(TYPE_S).z;
		samples++;
	}
	const double seconds = std::chrono::duration<double>(std::chrono::steady_clock::now()-start).count();
	print_info("TUBE cell "+to_string(h, 2u)+" cd "+to_string(sum/(double)samples/(0.5*u*u*d*length), 4u)+" seconds "+to_string(seconds, 1u));
}
