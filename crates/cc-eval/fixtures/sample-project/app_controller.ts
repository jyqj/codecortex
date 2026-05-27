import {
  Controller,
  Get,
  Post,
  Delete,
  Param,
  Body,
  Injectable,
  HttpException,
  HttpStatus,
} from '@nestjs/common';

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

@Injectable()
export class ItemsService {
  private items: Array<{ id: number; name: string }> = [
    { id: 1, name: 'Widget' },
    { id: 2, name: 'Gadget' },
  ];

  findAll() {
    return this.items;
  }

  findOne(id: number) {
    return this.items.find((item) => item.id === id) || null;
  }

  create(name: string) {
    const newItem = { id: this.items.length + 1, name };
    this.items.push(newItem);
    return newItem;
  }

  remove(id: number): boolean {
    const idx = this.items.findIndex((item) => item.id === id);
    if (idx === -1) return false;
    this.items.splice(idx, 1);
    return true;
  }
}

// ---------------------------------------------------------------------------
// Controller
// ---------------------------------------------------------------------------

@Controller('items')
export class ItemsController {
  constructor(private readonly itemsService: ItemsService) {}

  @Get()
  async findAll() {
    return this.itemsService.findAll();
  }

  @Get('/:id')
  async findOne(@Param('id') id: string) {
    const item = this.itemsService.findOne(Number(id));
    if (!item) {
      throw new HttpException('Item not found', HttpStatus.NOT_FOUND);
    }
    return item;
  }

  @Post()
  async create(@Body() body: { name: string }) {
    if (!body.name) {
      throw new HttpException('Name is required', HttpStatus.BAD_REQUEST);
    }
    return this.itemsService.create(body.name);
  }

  @Delete('/:id')
  async remove(@Param('id') id: string) {
    const deleted = this.itemsService.remove(Number(id));
    if (!deleted) {
      throw new HttpException('Item not found', HttpStatus.NOT_FOUND);
    }
    return { deleted: true };
  }
}
